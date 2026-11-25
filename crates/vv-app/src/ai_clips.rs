use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use base64::Engine;
use serde::{Deserialize, Serialize};
use vv_core::{
    CaptionWord, Clip, ClipId, ClipSource, CommandLabel, Id, InsertClip, LiftDelete, MediaId,
    Rational, SetCaptions, SubtitleTemplate, TimelineId, TrackKind,
};
use vv_transcribe::{Progress, Transcript};

use super::*;

const MODEL: &str = "gemini-3.5-flash";
/// Tried when `MODEL` stays overloaded (503) after the retries.
const FALLBACK_MODEL: &str = "gemini-flash-latest";
const SYSTEM: &str = "Select only compelling, self-contained short clips from this timestamped transcript. Prefer a strong opening, complete thought, natural ending, and novel moments. Avoid repetitions, intros and overlapping selections. Never invent speech or times. Return all worthwhile clips, but no filler. Times are milliseconds and must lie within the source duration.";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Suggestion {
    start_ms: u32,
    end_ms: u32,
    title: String,
    description: String,
    score: u8,
    reason: String,
    #[serde(skip, default = "selected_default")]
    selected: bool,
}

fn selected_default() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
struct Selection {
    prompt: String,
    duration: usize,
    model: String,
    clips: Vec<Suggestion>,
    #[serde(default)]
    verified: bool,
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    fingerprint: u64,
    duration_ms: u32,
    language: String,
    words: Vec<CaptionWord>,
    #[serde(default)]
    segments: Vec<TranscriptSegment>,
    selections: Vec<Selection>,
    #[serde(default)]
    frames: Vec<CachedFrame>,
}

#[derive(Serialize, Deserialize)]
struct TranscriptSegment {
    start_ms: u32,
    end_ms: u32,
    text: String,
}

fn segment_words(words: &[CaptionWord]) -> Vec<TranscriptSegment> {
    let mut segments = Vec::new();
    let mut current: Vec<&CaptionWord> = Vec::new();
    for word in words {
        if current
            .last()
            .is_some_and(|previous| word.start_ms.saturating_sub(previous.end_ms) > 800)
        {
            segments.push(TranscriptSegment {
                start_ms: current[0].start_ms,
                end_ms: current.last().unwrap().end_ms,
                text: current
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            });
            current.clear();
        }
        current.push(word);
        if word.text.ends_with(['.', '?', '!']) {
            segments.push(TranscriptSegment {
                start_ms: current[0].start_ms,
                end_ms: word.end_ms,
                text: current
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            });
            current.clear();
        }
    }
    if !current.is_empty() {
        segments.push(TranscriptSegment {
            start_ms: current[0].start_ms,
            end_ms: current.last().unwrap().end_ms,
            text: current
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
        });
    }
    segments
}

fn words_at_timeline_position(
    words: &[CaptionWord],
    source_start_ms: u32,
    source_end_ms: u32,
    timeline_start: vv_core::FrameIdx,
    fps: Rational,
) -> Vec<CaptionWord> {
    let offset_ms =
        (timeline_start as f64 * 1000.0 / fps.as_f64()).round() as i64 - i64::from(source_start_ms);
    words
        .iter()
        .filter(|w| w.start_ms >= source_start_ms && w.end_ms <= source_end_ms)
        .map(|w| CaptionWord {
            text: w.text.clone(),
            start_ms: (i64::from(w.start_ms) + offset_ms).max(0) as u32,
            end_ms: (i64::from(w.end_ms) + offset_ms).max(0) as u32,
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
struct CachedFrame {
    at_ms: u32,
    png_base64: String,
}

pub(crate) struct Job {
    media: MediaId,
    meta_fingerprint: u64,
    cancel: Arc<AtomicBool>,
    progress: Arc<Progress>,
    stage: Arc<std::sync::Mutex<&'static str>>,
    rx: mpsc::Receiver<Result<(Cache, usize), String>>,
}

struct SourceScan {
    media: MediaId,
    meta_fingerprint: u64,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<(u64, Cache), String>>,
}

#[derive(Default)]
pub(crate) struct State {
    /// `None` until checked; reset when the key changes in Settings.
    pub key_present: Option<bool>,
    pub prompt: String,
    pub duration: usize,
    pub subtitles: bool,
    pub message: Option<String>,
    pub results: Vec<Suggestion>,
    pub history: Vec<String>,
    pub history_index: usize,
    pub source: Option<MediaId>,
    source_choice: Option<MediaId>,
    source_fingerprint: Option<u64>,
    source_meta_fingerprint: Option<u64>,
    previews: HashMap<u32, egui::TextureHandle>,
    preview_selection: Option<(u64, usize)>,
    job: Option<Job>,
    source_scan: Option<SourceScan>,
    last_batch: Option<InsertedBatch>,
    replace_confirm: bool,
}

struct InsertedBatch {
    timeline: TimelineId,
    media: MediaId,
    media_clips: Vec<ClipId>,
    subtitle_clips: Vec<ClipId>,
    words: Vec<CaptionWord>,
}

fn cache_path(fingerprint: u64) -> Result<PathBuf, String> {
    let root = crate::library::Library::default_root()
        .ok_or("Cannot locate the application data directory")?;
    Ok(root
        .join("ai-clips")
        .join(format!("{fingerprint:016x}.json")))
}

#[cfg(test)]
use crate::gemini::{api_error, is_transient, load_key_at, save_key_at};
pub(crate) use crate::gemini::{delete_key, key_path, load_key, save_key};

fn source_fingerprint(path: &Path) -> Result<u64, String> {
    source_fingerprint_cancelled(path, &AtomicBool::new(false))
}

fn source_fingerprint_cancelled(path: &Path, cancel: &AtomicBool) -> Result<u64, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    let digest = hasher.finish();
    Ok(u64::from_le_bytes(digest.as_ref()[..8].try_into().unwrap()))
}

fn read_cache(fingerprint: u64, duration_ms: u32) -> Cache {
    cache_path(fingerprint)
        .ok()
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice::<Cache>(&bytes).ok())
        .filter(|cache| cache.fingerprint == fingerprint && cache.duration_ms == duration_ms)
        .unwrap_or(Cache {
            fingerprint,
            duration_ms,
            ..Cache::default()
        })
}

fn save_cache(cache: &Cache) -> Result<(), String> {
    static NEXT_TEMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = cache_path(cache.fingerprint)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temp, serde_json::to_vec(cache).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())
}

fn transcribe_source(
    path: &Path,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<Progress>,
) -> Result<Transcript, String> {
    let model_dir = crate::subtitles::models_dir().ok_or("Cannot locate speech models")?;
    let model = vv_transcribe::Model::preferred();
    let model_path = vv_transcribe::ensure_model(&model_dir, model, progress, cancel)
        .map_err(|e| format!("{e:?}"))?;
    let mut samples = Vec::new();
    let formats =
        vv_media::decode_audio_streams_streaming(path, &[0], Some(16_000), |_, channels, chunk| {
            if cancel.load(Ordering::Relaxed) {
                return std::ops::ControlFlow::Break(());
            }
            samples.extend(
                chunk
                    .chunks_exact(usize::from(channels))
                    .map(|frame| frame.iter().sum::<f32>() / f32::from(channels)),
            );
            std::ops::ControlFlow::Continue(())
        })
        .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if formats[0].is_none() {
        return Err("The source has no audio track".into());
    }
    vv_transcribe::transcribe(model, &model_path, &samples, None, progress, cancel)
        .map_err(|e| format!("{e:?}"))
}

fn gemini(
    cache: &Cache,
    prompt: &str,
    duration: usize,
    key: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Suggestion>, String> {
    let ranges = [(15, 30), (30, 60), (60, 90)];
    let (min, max) = ranges[duration.min(2)];
    // Sentences, not words: a long source sent word by word is a huge
    // request that Gemini often rejects as overloaded. `refine` snaps the
    // times back to the words.
    let transcript: Vec<_> = cache
        .segments
        .iter()
        .map(|s| serde_json::json!([s.start_ms, s.end_ms, s.text]))
        .collect();
    let request = serde_json::json!({
        "systemInstruction": {"parts": [{"text": SYSTEM}]},
        "contents": [{"parts": [{"text": format!("Source duration: {} ms. Clip duration: {}-{} seconds. User instructions: {}\nTranscript [start_ms,end_ms,sentence]: {}", cache.duration_ms, min, max, prompt, serde_json::to_string(&transcript).unwrap())}]}],
        "generationConfig": {"responseMimeType": "application/json", "responseSchema": {"type": "OBJECT", "properties": {"clips": {"type": "ARRAY", "items": {"type": "OBJECT", "properties": {"start_ms": {"type": "INTEGER"}, "end_ms": {"type": "INTEGER"}, "title": {"type": "STRING"}, "description": {"type": "STRING"}, "score": {"type": "INTEGER"}, "reason": {"type": "STRING"}}, "required": ["start_ms", "end_ms", "title", "description", "score", "reason"]}}}, "required": ["clips"]}}
    });
    let body = generate(&request, key, cancel)?;
    let text = body
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|v| v.as_str())
        .ok_or("Gemini returned no clips")?;
    parse_suggestions(text)
}

fn generate(
    request: &serde_json::Value,
    key: &str,
    cancel: &AtomicBool,
) -> Result<serde_json::Value, String> {
    crate::gemini::generate(request, key, cancel, &[MODEL, FALLBACK_MODEL], true)
}

fn parse_suggestions(text: &str) -> Result<Vec<Suggestion>, String> {
    let parsed: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    serde_json::from_value(parsed.get("clips").cloned().ok_or("Missing clips array")?)
        .map_err(|e| e.to_string())
}

fn frame_at(path: &Path, at_ms: u32) -> Result<CachedFrame, String> {
    let png_data = crate::ai_context::frame_png(path, f64::from(at_ms) / 1000.0)?;
    Ok(CachedFrame {
        at_ms,
        png_base64: base64::engine::general_purpose::STANDARD.encode(png_data),
    })
}

fn preview_texture(ctx: &egui::Context, frame: &CachedFrame) -> Option<egui::TextureHandle> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&frame.png_base64)
        .ok()?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut rgba = vec![0; decoder.output_buffer_size()?];
    let info = decoder.next_frame(&mut rgba).ok()?;
    if info.color_type != png::ColorType::Rgba {
        return None;
    }
    Some(ctx.load_texture(
        format!("ai-clip-{}", frame.at_ms),
        egui::ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &rgba[..info.buffer_size()],
        ),
        egui::TextureOptions::LINEAR,
    ))
}

fn verify_visual(
    cache: &mut Cache,
    path: &Path,
    clips: &mut [Suggestion],
    key: &str,
    cancel: &AtomicBool,
) -> Result<(), String> {
    if clips.is_empty() {
        return Ok(());
    }
    let mut parts = vec![
        serde_json::json!({"text": "Check each indexed clip against the frame from its midpoint. Is the visual content consistent with its spoken text? Score visual interest 0-100. Return one result per index. Do not infer unseen events."}),
    ];
    for (index, clip) in clips.iter().take(8).enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let midpoint = clip.start_ms + (clip.end_ms - clip.start_ms) / 2;
        let frame = if let Some(frame) = cache
            .frames
            .iter()
            .find(|f| f.at_ms.abs_diff(midpoint) < 500)
        {
            frame
        } else {
            cache.frames.push(frame_at(path, midpoint)?);
            cache.frames.last().unwrap()
        };
        let speech = cache
            .words
            .iter()
            .filter(|w| w.start_ms >= clip.start_ms && w.end_ms <= clip.end_ms)
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        parts.push(serde_json::json!({"text": format!("Clip {index}: {}-{} ms. Speech: {}", clip.start_ms, clip.end_ms, speech)}));
        parts.push(
            serde_json::json!({"inlineData": {"mimeType": "image/png", "data": frame.png_base64}}),
        );
    }
    save_cache(cache)?;
    let request = serde_json::json!({
        "contents": [{"parts": parts}],
        "generationConfig": {"responseMimeType": "application/json", "responseSchema": {"type": "OBJECT", "properties": {"clips": {"type": "ARRAY", "items": {"type": "OBJECT", "properties": {"index": {"type": "INTEGER"}, "visual_score": {"type": "INTEGER"}, "coherent": {"type": "BOOLEAN"}}, "required": ["index", "visual_score", "coherent"]}}}, "required": ["clips"]}}
    });
    let body = generate(&request, key, cancel)?;
    let content = body
        .pointer("/candidates/0/content/parts/0/text")
        .and_then(|v| v.as_str())
        .ok_or("Gemini returned no visual analysis")?;
    let parsed: serde_json::Value = serde_json::from_str(content).map_err(|e| e.to_string())?;
    for result in parsed["clips"]
        .as_array()
        .ok_or("Invalid visual analysis")?
    {
        let Some(index) = result["index"].as_u64().map(|n| n as usize) else {
            continue;
        };
        let Some(score) = result["visual_score"].as_u64() else {
            continue;
        };
        let Some(clip) = clips.get_mut(index) else {
            continue;
        };
        if score > 100 {
            continue;
        }
        clip.score = (u64::from(clip.score) * 8 / 10 + score * 2 / 10) as u8;
        if result["coherent"] == false {
            clip.score = clip.score.min(20);
        }
    }
    clips.sort_by_key(|c| std::cmp::Reverse(c.score));
    Ok(())
}

fn refine(
    mut clips: Vec<Suggestion>,
    words: &[CaptionWord],
    duration_ms: u32,
    range: usize,
) -> Vec<Suggestion> {
    let bounds = [(15_000, 30_000), (30_000, 60_000), (60_000, 90_000)][range.min(2)];
    clips.retain_mut(|clip| {
        if clip.start_ms >= clip.end_ms || clip.end_ms > duration_ms || clip.score > 100 {
            return false;
        }
        let start = words
            .iter()
            .filter(|w| {
                w.start_ms >= clip.start_ms.saturating_sub(700)
                    && w.start_ms <= clip.start_ms.saturating_add(700)
            })
            .min_by_key(|w| w.start_ms.abs_diff(clip.start_ms));
        let end = words
            .iter()
            .filter(|w| {
                w.end_ms >= clip.end_ms.saturating_sub(700)
                    && w.end_ms <= clip.end_ms.saturating_add(700)
            })
            .min_by_key(|w| w.end_ms.abs_diff(clip.end_ms));
        if let Some(w) = start {
            clip.start_ms = w.start_ms;
        }
        if let Some(w) = end {
            clip.end_ms = w.end_ms;
        }
        if let Some((index, _)) = words.iter().enumerate().rfind(|(_, w)| {
            w.end_ms <= clip.start_ms
                && clip.start_ms - w.end_ms <= 1_500
                && w.text.ends_with(['.', '?', '!'])
        }) && let Some(next) = words.get(index + 1)
            && next.start_ms < clip.end_ms
            && (clip.end_ms - next.start_ms) >= bounds.0
        {
            clip.start_ms = next.start_ms;
        }
        if let Some(w) = words.iter().find(|w| {
            w.end_ms >= clip.end_ms
                && w.end_ms - clip.end_ms <= 1_500
                && w.text.ends_with(['.', '?', '!'])
        }) && w.end_ms - clip.start_ms <= bounds.1
        {
            clip.end_ms = w.end_ms;
        }
        clip.selected = true;
        bounds.0 <= clip.end_ms.saturating_sub(clip.start_ms)
            && clip.end_ms - clip.start_ms <= bounds.1
            && !clip.title.trim().is_empty()
    });
    clips.sort_by_key(|c| std::cmp::Reverse(c.score));
    let mut selected: Vec<Suggestion> = Vec::new();
    for clip in clips {
        if selected.iter().all(|other| {
            let overlap = clip
                .end_ms
                .min(other.end_ms)
                .saturating_sub(clip.start_ms.max(other.start_ms));
            overlap < (clip.end_ms - clip.start_ms).min(other.end_ms - other.start_ms) / 3
        }) {
            selected.push(clip);
        }
    }
    selected
}

impl VenturiApp {
    pub(crate) fn mcp_start_ai_clips(&mut self, media: MediaId, prompt: String, duration: usize) {
        self.ai_clips.source_choice = Some(media);
        self.ai_clips.prompt = prompt;
        self.ai_clips.duration = duration;
        self.start_ai_clips();
    }

    pub(crate) fn mcp_ai_clips_status(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.ai_clips.job.is_some(),
            "message": self.ai_clips.message,
            "source_media_id": self.ai_clips.source.map(|id| id.raw().to_string()),
            "suggestions": self.ai_clips.results.iter().enumerate().map(|(index, clip)| serde_json::json!({
                "index": index,
                "start_ms": clip.start_ms,
                "end_ms": clip.end_ms,
                "title": clip.title,
                "description": clip.description,
                "score": clip.score,
                "reason": clip.reason,
                "selected": clip.selected,
            })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn mcp_insert_ai_clips(&mut self, replace_previous: bool) {
        self.insert_ai_clips(replace_previous);
    }

    pub(crate) fn mcp_select_ai_clips(&mut self, indices: &[usize]) {
        for (index, suggestion) in self.ai_clips.results.iter_mut().enumerate() {
            suggestion.selected = indices.contains(&index);
        }
    }
    fn sync_ai_clips_source(&mut self, ctx: &egui::Context) {
        let Some(media) = self.ai_clips.source_choice else {
            return;
        };
        if self.ai_clips.job.is_some() {
            return;
        }
        let Some(item) = self.session.project.media_pool.get(media) else {
            return;
        };
        if !item.meta.has_video || item.compound.is_some() {
            return;
        }
        let Ok(meta_fingerprint) = vv_media::content_fingerprint(&item.path) else {
            return;
        };
        if self.ai_clips.source == Some(media)
            && self.ai_clips.source_meta_fingerprint == Some(meta_fingerprint)
        {
            return;
        }
        if let Some(scan) = &self.ai_clips.source_scan
            && scan.media == media
            && scan.meta_fingerprint == meta_fingerprint
        {
            match scan.rx.try_recv() {
                Ok(Ok((fingerprint, cache))) => {
                    self.ai_clips.source_scan = None;
                    self.ai_clips.source = Some(media);
                    self.ai_clips.source_fingerprint = Some(fingerprint);
                    self.ai_clips.source_meta_fingerprint = Some(meta_fingerprint);
                    self.ai_clips.previews.clear();
                    self.ai_clips.preview_selection = None;
                    self.ai_clips.history = cache
                        .selections
                        .iter()
                        .map(|s| {
                            format!(
                                "{}–{} s · {}",
                                [(15, 30), (30, 60), (60, 90)][s.duration.min(2)].0,
                                [(15, 30), (30, 60), (60, 90)][s.duration.min(2)].1,
                                if s.prompt.is_empty() {
                                    "Automatic"
                                } else {
                                    &s.prompt
                                }
                            )
                        })
                        .collect();
                    self.ai_clips.history_index = cache.selections.len().saturating_sub(1);
                    self.ai_clips.results = cache
                        .selections
                        .last()
                        .map(|s| s.clips.clone())
                        .unwrap_or_default();
                }
                Ok(Err(error)) => {
                    self.ai_clips.message = Some(error);
                    self.ai_clips.source_scan = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100))
                }
                Err(_) => self.ai_clips.source_scan = None,
            }
            return;
        }
        if let Some(scan) = self.ai_clips.source_scan.take() {
            scan.cancel.store(true, Ordering::Relaxed);
        }
        self.ai_clips.source = None;
        self.ai_clips.source_fingerprint = None;
        self.ai_clips.source_meta_fingerprint = None;
        self.ai_clips.results.clear();
        self.ai_clips.history.clear();
        self.ai_clips.previews.clear();
        self.ai_clips.preview_selection = None;
        let duration_ms =
            (item.meta.duration_frames as f64 * 1000.0 / item.meta.fps.as_f64()).round() as u32;
        let path = item.path.clone();
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::spawn(move || {
            let result = source_fingerprint_cancelled(&path, &worker_cancel)
                .map(|fingerprint| (fingerprint, read_cache(fingerprint, duration_ms)));
            let _ = tx.send(result);
        });
        self.ai_clips.source_scan = Some(SourceScan {
            media,
            meta_fingerprint,
            cancel,
            rx,
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }

    pub(crate) fn start_ai_clips(&mut self) {
        if self.ai_clips.job.is_some() {
            return;
        }
        if let Some(scan) = self.ai_clips.source_scan.take() {
            scan.cancel.store(true, Ordering::Relaxed);
        }
        let key = load_key()
            .ok()
            .filter(|key| !key.is_empty())
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .unwrap_or_default();
        if key.trim().is_empty() {
            self.ai_clips.key_present = Some(false);
            self.ai_clips.message = Some(t!("ai_clips.key_required").into_owned());
            return;
        }
        let Some(media) = self.ai_clips.source_choice else {
            self.ai_clips.message = Some("Select one video in the media pool".into());
            return;
        };
        let Some(item) = self.session.project.media_pool.get(media) else {
            return;
        };
        if !item.meta.has_video || !item.meta.has_audio || item.compound.is_some() {
            self.ai_clips.message = Some("Select a video with audio".into());
            return;
        }
        let Ok(meta_fingerprint) = vv_media::content_fingerprint(&item.path) else {
            self.ai_clips.message = Some("Source file is unavailable".into());
            return;
        };
        let duration_ms =
            (item.meta.duration_frames as f64 * 1000.0 / item.meta.fps.as_f64()).round() as u32;
        let path = item.path.clone();
        let prompt = self.ai_clips.prompt.clone();
        let duration = self.ai_clips.duration;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Progress::default());
        let stage = Arc::new(std::sync::Mutex::new("Preparing source"));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn({
            let cancel = cancel.clone();
            let progress = progress.clone();
            let stage = stage.clone();
            move || {
                let result = (|| {
                    let fingerprint = source_fingerprint_cancelled(&path, &cancel)?;
                    let mut cache = read_cache(fingerprint, duration_ms);
                    if cache.words.is_empty() {
                        *stage.lock().unwrap() = "Transcribing source";
                        let transcript = transcribe_source(&path, &cancel, &progress)?;
                        cache.language = transcript.language;
                        cache.words = transcript
                            .words
                            .into_iter()
                            .map(|w| CaptionWord {
                                text: w.text,
                                start_ms: w.start_ms,
                                end_ms: w.end_ms,
                            })
                            .collect();
                        cache.segments = segment_words(&cache.words);
                        save_cache(&cache)?;
                    }
                    if cache.segments.is_empty() && !cache.words.is_empty() {
                        cache.segments = segment_words(&cache.words);
                        save_cache(&cache)?;
                    }
                    if cache.words.is_empty() {
                        return Err("No speech found".into());
                    }
                    if cancel.load(Ordering::Relaxed) {
                        return Err("Cancelled".into());
                    }
                    let existing = cache.selections.iter().position(|s| {
                        s.prompt == prompt && s.duration == duration && s.model == MODEL
                    });
                    let index = if let Some(index) = existing {
                        index
                    } else {
                        *stage.lock().unwrap() = "Analyzing transcript with Gemini";
                        let proposed = gemini(&cache, &prompt, duration, &key, &cancel)?;
                        let clips = refine(proposed, &cache.words, duration_ms, duration);
                        cache.selections.push(Selection {
                            prompt,
                            duration,
                            model: MODEL.into(),
                            clips,
                            verified: false,
                        });
                        save_cache(&cache)?;
                        cache.selections.len() - 1
                    };
                    if !cache.selections[index].verified {
                        *stage.lock().unwrap() = "Checking candidate video frames";
                        let mut clips = cache.selections[index].clips.clone();
                        // Optional pass: the transcript clips stay usable without it.
                        match verify_visual(&mut cache, &path, &mut clips, &key, &cancel) {
                            Ok(()) => {
                                cache.selections[index].clips = clips;
                                cache.selections[index].verified = true;
                                save_cache(&cache)?;
                            }
                            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
                            Err(_) => {}
                        }
                    }
                    if source_fingerprint_cancelled(&path, &cancel)? != fingerprint {
                        return Err("Source video changed during analysis".into());
                    }
                    Ok((cache, index))
                })();
                let _ = tx.send(result);
            }
        });
        self.ai_clips.job = Some(Job {
            media,
            meta_fingerprint,
            cancel,
            progress,
            stage,
            rx,
        });
        self.ai_clips.message = None;
    }

    pub(crate) fn poll_ai_clips(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.ai_clips.job else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(_) => Err("AI Clips worker stopped".into()),
        };
        let job = self.ai_clips.job.take().unwrap();
        if self
            .session
            .project
            .media_pool
            .get(job.media)
            .is_none_or(|item| {
                vv_media::content_fingerprint(&item.path).ok() != Some(job.meta_fingerprint)
            })
        {
            self.ai_clips.message = Some("Source video changed during analysis".into());
            return;
        }
        match result {
            Ok((cache, index)) => {
                self.ai_clips.source = Some(job.media);
                self.ai_clips.source_fingerprint = Some(cache.fingerprint);
                self.ai_clips.source_meta_fingerprint = self
                    .session
                    .project
                    .media_pool
                    .get(job.media)
                    .and_then(|item| vv_media::content_fingerprint(&item.path).ok());
                self.ai_clips.previews.clear();
                self.ai_clips.preview_selection = None;
                self.ai_clips.history = cache
                    .selections
                    .iter()
                    .map(|s| {
                        format!(
                            "{}–{} s · {}",
                            [(15, 30), (30, 60), (60, 90)][s.duration.min(2)].0,
                            [(15, 30), (30, 60), (60, 90)][s.duration.min(2)].1,
                            if s.prompt.is_empty() {
                                "Automatic"
                            } else {
                                &s.prompt
                            }
                        )
                    })
                    .collect();
                self.ai_clips.history_index = index;
                self.ai_clips.results = cache.selections[index].clips.clone();
                self.ai_clips.message =
                    Some(format!("{} clips ready", self.ai_clips.results.len()));
            }
            Err(error) => self.ai_clips.message = Some(error),
        }
    }

    pub(crate) fn show_ai_clips_tab(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("ai_clips_tab")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                self.show_ai_clips_contents(ui);
            });
    }

    fn show_ai_clips_contents(&mut self, ui: &mut egui::Ui) {
        if self.ai_clips.key_present.is_none() {
            self.ai_clips.key_present = Some(
                load_key().is_ok_and(|key| !key.is_empty())
                    || std::env::var("GEMINI_API_KEY").is_ok_and(|key| !key.trim().is_empty()),
            );
        }
        let videos: Vec<(MediaId, String)> = self
            .session
            .project
            .media_pool
            .iter()
            .filter(|(_, item)| {
                item.meta.has_video && item.meta.has_audio && item.compound.is_none()
            })
            .map(|(id, item)| {
                (
                    id,
                    item.path.file_name().map_or_else(
                        || item.path.display().to_string(),
                        |name| name.to_string_lossy().into_owned(),
                    ),
                )
            })
            .collect();
        if self
            .ai_clips
            .source_choice
            .is_some_and(|choice| !videos.iter().any(|(id, _)| *id == choice))
        {
            self.ai_clips.source_choice = None;
        }
        if self.ai_clips.source_choice.is_none() {
            self.ai_clips.source_choice = self
                .media_pool_state
                .single_selected()
                .copied()
                .filter(|id| videos.iter().any(|(video, _)| video == id))
                .or_else(|| videos.first().map(|(id, _)| *id));
        }
        if self.ai_clips.source_choice.is_none() {
            self.ai_clips.source = None;
            self.ai_clips.results.clear();
            self.ai_clips.history.clear();
        }
        self.sync_ai_clips_source(ui.ctx());
        if let (Some(media), Some(fingerprint)) =
            (self.ai_clips.source, self.ai_clips.source_fingerprint)
            && self.ai_clips.preview_selection != Some((fingerprint, self.ai_clips.history_index))
            && let Some(item) = self.session.project.media_pool.get(media)
        {
            let duration_ms =
                (item.meta.duration_frames as f64 * 1000.0 / item.meta.fps.as_f64()).round() as u32;
            let cache = read_cache(fingerprint, duration_ms);
            for frame in cache.frames.iter().filter(|frame| {
                self.ai_clips.results.iter().any(|clip| {
                    frame
                        .at_ms
                        .abs_diff(clip.start_ms + (clip.end_ms - clip.start_ms) / 2)
                        < 500
                })
            }) {
                if !self.ai_clips.previews.contains_key(&frame.at_ms)
                    && let Some(texture) = preview_texture(ui.ctx(), frame)
                {
                    self.ai_clips.previews.insert(frame.at_ms, texture);
                }
            }
            self.ai_clips.preview_selection = Some((fingerprint, self.ai_clips.history_index));
        }
        crate::ui_kit::heading(ui, t!("ai_clips.title"));
        ui.label(t!("ai_clips.source"));
        if videos.is_empty() {
            ui.weak(t!("ai_clips.select_source"));
        }
        self.ai_clips_source_tiles(ui, &videos);
        if self.ai_clips.key_present == Some(false) {
            ui.add_space(theme::SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(theme::ACCENT, t!("ai_clips.key_missing"));
                if ui.button(t!("ai_clips.open_settings")).clicked() {
                    self.open_settings(settings_dialog::Section::Integrations);
                }
            });
        }
        ui.add_space(theme::SPACE_2);
        ui.horizontal(|ui| {
            for (index, label) in ["15–30 s", "30–60 s", "60–90 s"].iter().enumerate() {
                ui.selectable_value(&mut self.ai_clips.duration, index, *label);
            }
        });
        ui.label(t!("ai_clips.instructions"));
        ui.add(
            egui::TextEdit::multiline(&mut self.ai_clips.prompt)
                .desired_rows(3)
                .desired_width(ui.available_width())
                .hint_text(t!("ai_clips.prompt_hint")),
        );
        ui.checkbox(&mut self.ai_clips.subtitles, t!("ai_clips.subtitles"));
        if let Some(job) = &self.ai_clips.job {
            let stage = *job.stage.lock().unwrap();
            ui.label(match stage {
                "Preparing source" => t!("ai_clips.stage_prepare"),
                "Transcribing source" => t!("ai_clips.stage_transcribe"),
                "Analyzing transcript with Gemini" => t!("ai_clips.stage_analyze"),
                "Checking candidate video frames" => t!("ai_clips.stage_visual"),
                _ => t!("ai_clips.stage_prepare"),
            });
            let fraction = match stage {
                "Preparing source" => 0.02,
                "Transcribing source" => 0.05 + job.progress.fraction() * 0.65,
                "Analyzing transcript with Gemini" => 0.75,
                "Checking candidate video frames" => 0.9,
                _ => 0.0,
            };
            ui.add(egui::ProgressBar::new(fraction));
            if ui.button(t!("ai_clips.cancel")).clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
        } else if ui
            .button(if self.ai_clips.results.is_empty() {
                t!("ai_clips.generate")
            } else {
                t!("ai_clips.regenerate")
            })
            .clicked()
        {
            self.start_ai_clips();
        }
        if let Some(message) = &self.ai_clips.message {
            ui.label(message);
        }
        if !self.ai_clips.history.is_empty() {
            let mut selected = self.ai_clips.history_index;
            egui::ComboBox::from_label(t!("ai_clips.previous"))
                .selected_text(&self.ai_clips.history[selected])
                .show_ui(ui, |ui| {
                    for (i, name) in self.ai_clips.history.iter().enumerate() {
                        ui.selectable_value(&mut selected, i, name);
                    }
                });
            if selected != self.ai_clips.history_index {
                self.load_ai_clips_history(selected);
            }
        }
        for clip in &mut self.ai_clips.results {
            ui.horizontal(|ui| {
                ui.checkbox(&mut clip.selected, "");
                let midpoint = clip.start_ms + (clip.end_ms - clip.start_ms) / 2;
                if let Some((_, texture)) = self
                    .ai_clips
                    .previews
                    .iter()
                    .find(|(at, _)| at.abs_diff(midpoint) < 500)
                {
                    ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(96.0, 54.0)));
                }
                ui.label(format!(
                    "{} · {:.1}s · {} / 100",
                    clip.title,
                    (clip.end_ms - clip.start_ms) as f32 / 1000.0,
                    clip.score
                ));
            });
            ui.small(&clip.description);
            ui.weak(&clip.reason);
        }
        if !self.ai_clips.results.is_empty() && ui.button(t!("ai_clips.insert")).clicked() {
            self.insert_ai_clips(false);
        }
        if self.ai_clips.last_batch.is_some() && !self.ai_clips.results.is_empty() {
            if ui.button(t!("ai_clips.replace")).clicked() {
                self.ai_clips.replace_confirm = true;
            }
            if self.ai_clips.replace_confirm {
                ui.label(t!("ai_clips.replace_warning"));
                ui.horizontal(|ui| {
                    if ui.button(t!("ai_clips.confirm_replace")).clicked() {
                        self.ai_clips.replace_confirm = false;
                        self.insert_ai_clips(true);
                    }
                    if ui.button(t!("ai_clips.cancel")).clicked() {
                        self.ai_clips.replace_confirm = false;
                    }
                });
            }
        }
    }

    fn ai_clips_source_tiles(&mut self, ui: &mut egui::Ui, videos: &[(MediaId, String)]) {
        const MIN_TILE_W: f32 = 110.0;
        let margin = 8.0;
        let spacing = ui.spacing().item_spacing.x;
        let available = ui.available_width();
        let columns = ((available + spacing) / (MIN_TILE_W + margin + spacing))
            .floor()
            .max(1.0);
        let tile_w = ((available + spacing) / columns - spacing - margin).floor();
        let mut clicked = None;
        ui.horizontal_wrapped(|ui| {
            for (id, name) in videos {
                let Some(item) = self.session.project.media_pool.get(*id) else {
                    continue;
                };
                let thumb = if self.media_pool_state.is_offline(id) {
                    pool_view::Thumb::Offline
                } else {
                    match self.thumbnails.get(&item.content_hash).cloned().flatten() {
                        Some(texture) => pool_view::Thumb::Texture(texture),
                        None => pool_view::Thumb::None,
                    }
                };
                let response = pool_view::media_tile(
                    ui,
                    ui.id().with("ai_clips_source").with(*id),
                    &thumb,
                    &item.meta,
                    name,
                    tile_w,
                    self.ai_clips.source_choice == Some(*id),
                );
                if response.clicked() {
                    clicked = Some(*id);
                }
            }
        });
        if let Some(id) = clicked
            && self.ai_clips.job.is_none()
        {
            self.ai_clips.source_choice = Some(id);
        }
    }

    fn load_ai_clips_history(&mut self, index: usize) {
        let Some(media) = self.ai_clips.source else {
            return;
        };
        let Some(item) = self.session.project.media_pool.get(media) else {
            return;
        };
        let duration_ms =
            (item.meta.duration_frames as f64 * 1000.0 / item.meta.fps.as_f64()).round() as u32;
        let Some(fingerprint) = self.ai_clips.source_fingerprint else {
            return;
        };
        let cache = read_cache(fingerprint, duration_ms);
        if let Some(selection) = cache.selections.get(index) {
            self.ai_clips.history_index = index;
            self.ai_clips.results = selection.clips.clone();
        }
    }

    fn insert_ai_clips(&mut self, replace: bool) {
        let (Some(media), Some(timeline_id)) = (self.ai_clips.source, self.timeline_id) else {
            return;
        };
        let Some(item) = self.session.project.media_pool.get(media) else {
            return;
        };
        let meta = item.meta.clone();
        let Ok(fingerprint) = source_fingerprint(&item.path) else {
            return;
        };
        if self.ai_clips.source_fingerprint != Some(fingerprint) {
            self.ai_clips.message = Some("Source video changed; regenerate clips".into());
            return;
        }
        let chosen: Vec<_> = self
            .ai_clips
            .results
            .iter()
            .filter(|c| c.selected)
            .cloned()
            .collect();
        if chosen.is_empty() {
            return;
        }
        let previous = if replace {
            self.ai_clips.last_batch.as_ref()
        } else {
            None
        };
        let mut old_media = Vec::new();
        let mut old_captions = None;
        if let Some(batch) = previous {
            if batch.timeline != timeline_id || batch.media != media {
                self.ai_clips.message =
                    Some("The previous clips belong to another source or timeline".into());
                return;
            }
            let timeline = &self.session.project.timelines[timeline_id];
            for id in &batch.media_clips {
                let Some((track, _)) = timeline
                    .tracks
                    .iter()
                    .enumerate()
                    .find(|(_, t)| t.clips.iter().any(|c| c.id == *id))
                else {
                    self.ai_clips.message =
                        Some("Previous clips were changed; replacement was cancelled".into());
                    return;
                };
                old_media.push((track, *id));
            }
            if !batch.words.is_empty() {
                let Some(mut captions) = self.session.project.captions.get(&timeline_id).cloned()
                else {
                    return;
                };
                let Some(track) = vv_core::captions::subtitle_track(timeline) else {
                    return;
                };
                let mut clips = timeline.tracks[track].clips.clone();
                for id in &batch.subtitle_clips {
                    let Some(index) = clips.iter().position(|c| c.id == *id) else {
                        self.ai_clips.message = Some(
                            "Previous subtitles were changed; replacement was cancelled".into(),
                        );
                        return;
                    };
                    clips.remove(index);
                }
                for word in &batch.words {
                    let Some(index) = captions.words.iter().position(|w| w == word) else {
                        self.ai_clips.message = Some(
                            "Previous subtitles were changed; replacement was cancelled".into(),
                        );
                        return;
                    };
                    captions.words.remove(index);
                }
                old_captions = Some((captions, clips));
            }
        }
        let fps = self.session.project.timelines[timeline_id].fps;
        let mark = self.session.history.begin_group();
        for (track, id) in old_media {
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(LiftDelete::new(timeline_id, track, id)),
            );
        }
        if let Some((captions, clips)) = old_captions {
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(SetCaptions::new(timeline_id, Some(captions), clips)),
            );
        }
        let mut at = self.session.project.timelines[timeline_id].total_frames();
        let mut batch = InsertedBatch {
            timeline: timeline_id,
            media,
            media_clips: Vec::new(),
            subtitle_clips: Vec::new(),
            words: Vec::new(),
        };
        let existing_video_track = self.session.project.timelines[timeline_id]
            .tracks_of_kind(TrackKind::Video)
            .find(|(_, track)| !track.locked)
            .map(|(index, _)| index);
        let video_track = existing_video_track.unwrap_or_else(|| {
            vv_core::edit::add_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                TrackKind::Video,
            )
        });
        let existing_audio_track = self.session.project.timelines[timeline_id]
            .tracks_of_kind(TrackKind::Audio)
            .find(|(_, track)| !track.locked)
            .map(|(index, _)| index);
        let audio_track = if meta.has_audio {
            Some(existing_audio_track.unwrap_or_else(|| {
                vv_core::edit::add_track(
                    &mut self.session.project,
                    &mut self.session.history,
                    timeline_id,
                    TrackKind::Audio,
                )
            }))
        } else {
            None
        };
        let rate = Rational::conform_rate(fps, meta.fps);
        let subtitle_template = self
            .subtitles
            .selected
            .clone()
            .filter(|id| SubtitleTemplate::find(id).is_some())
            .unwrap_or_else(|| vv_core::captions::DEFAULT_TEMPLATE.into());
        let mut caption_words = Vec::new();
        let mut subtitle_clips = Vec::new();
        let duration_ms = (meta.duration_frames as f64 * 1000.0 / meta.fps.as_f64()).round() as u32;
        let cache = read_cache(fingerprint, duration_ms);
        for selection in chosen {
            let start = (selection.start_ms as f64 * meta.fps.as_f64() / 1000.0).round() as i64;
            let end = (selection.end_ms as f64 * meta.fps.as_f64() / 1000.0).round() as i64;
            let group = self.session.project.alloc_link_group_id();
            let mut clip = Clip::from_source_range(
                self.session.project.alloc_clip_id(),
                ClipSource::Media(media),
                start,
                end,
                at,
                rate,
            );
            clip.linked_group = Some(group);
            let len = clip.timeline_len;
            if len <= 0 {
                continue;
            }
            batch.media_clips.push(clip.id);
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(InsertClip {
                    timeline: timeline_id,
                    track_index: video_track,
                    clip,
                }),
            );
            if let Some(track_index) = audio_track {
                let mut audio = Clip::from_source_range(
                    self.session.project.alloc_clip_id(),
                    ClipSource::Media(media),
                    start,
                    end,
                    at,
                    rate,
                );
                audio.linked_group = Some(group);
                batch.media_clips.push(audio.id);
                self.session.history.do_command(
                    &mut self.session.project,
                    Box::new(InsertClip {
                        timeline: timeline_id,
                        track_index,
                        clip: audio,
                    }),
                );
            }
            if self.ai_clips.subtitles {
                let words = words_at_timeline_position(
                    &cache.words,
                    selection.start_ms,
                    selection.end_ms,
                    at,
                    fps,
                );
                if let Some(template) = SubtitleTemplate::find(&subtitle_template) {
                    let mut clips = vv_core::captions::build_clips(
                        &mut self.session.project,
                        timeline_id,
                        &words,
                        template,
                    );
                    for clip in &mut clips {
                        clip.linked_group = Some(group);
                        batch.subtitle_clips.push(clip.id);
                    }
                    subtitle_clips.extend(clips);
                }
                caption_words.extend(words);
            }
            at += len;
        }
        if !caption_words.is_empty() {
            batch.words = caption_words.clone();
            let mut captions = self
                .session
                .project
                .captions
                .get(&timeline_id)
                .cloned()
                .unwrap_or_default();
            captions.words.extend(caption_words);
            if captions.template.is_empty() {
                captions.template = subtitle_template;
            }
            if let Some(track) =
                vv_core::captions::subtitle_track(&self.session.project.timelines[timeline_id])
            {
                subtitle_clips.extend(
                    self.session.project.timelines[timeline_id].tracks[track]
                        .clips
                        .iter()
                        .cloned(),
                );
            }
            subtitle_clips.sort_by_key(|c| c.timeline_start);
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(SetCaptions::new(
                    timeline_id,
                    Some(captions),
                    subtitle_clips,
                )),
            );
        }
        self.session
            .history
            .end_group_as(mark, CommandLabel::InsertClips);
        self.ai_clips.last_batch = Some(batch);
    }
}

#[cfg(test)]
#[path = "tests/ai_clips.rs"]
mod tests;

pub(crate) fn cached_words(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<Vec<CaptionWord>>, String> {
    let Some(root) = crate::library::Library::default_root() else {
        return Ok(None);
    };
    if !root.join("ai-clips").is_dir() {
        return Ok(None);
    }
    let fingerprint = source_fingerprint_cancelled(path, cancel)?;
    let cache_file = cache_path(fingerprint)?;
    let file = match std::fs::File::open(cache_file) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let cache: Cache =
        serde_json::from_reader(file.take(64 * 1024 * 1024)).map_err(|e| e.to_string())?;
    if cache.fingerprint != fingerprint {
        return Ok(None);
    }
    Ok(Some(cache.words))
}
