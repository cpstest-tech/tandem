use super::Job;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use vv_core::{ClipSource, FrameIdx, Project, TimelineId, TrackKind};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Transcript {
        path: PathBuf,
        source_seconds: f64,
        source_duration: f64,
        at_ms: u64,
        duration_ms: u64,
    },
    Frame {
        path: PathBuf,
        source_seconds: f64,
        at_ms: u64,
    },
    Audio {
        path: PathBuf,
        source_seconds: f64,
        source_duration: f64,
        at_ms: u64,
        duration_ms: u64,
        stream: usize,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attachment {
    pub file: String,
    pub mime: String,
    pub at_ms: u64,
    pub description: String,
}

pub fn snapshot(
    project: &Project,
    timeline_id: TimelineId,
    start: FrameIdx,
    end: FrameIdx,
    frames: bool,
    audio: bool,
) -> Vec<Source> {
    let Some(timeline) = project.timelines.get(timeline_id) else {
        return Vec::new();
    };
    if start >= end {
        return Vec::new();
    }
    let fps = timeline.fps.as_f64();
    let mut sources = Vec::new();
    let has_audio = timeline.tracks.iter().any(|track| {
        track.kind == TrackKind::Audio
            && track
                .clips
                .iter()
                .any(|clip| clip.timeline_start < end && clip.timeline_end() > start)
    });
    for track in timeline.tracks.iter().filter(|track| {
        !track.muted
            && if has_audio {
                track.kind == TrackKind::Audio
            } else {
                track.kind == TrackKind::Video
            }
    }) {
        for clip in track
            .clips
            .iter()
            .filter(|clip| !clip.disabled && clip.freeze.is_none())
        {
            if sources.len() >= 64 {
                break;
            }
            let first = start.max(clip.timeline_start);
            let last = end.min(clip.timeline_end());
            if first >= last {
                continue;
            }
            let ClipSource::Media(id) = clip.source else {
                continue;
            };
            let Some(item) = project.media_pool.get(id) else {
                continue;
            };
            if item.compound.is_some() || !item.meta.has_audio {
                continue;
            }
            let source_seconds = clip.media_secs_at(first, fps);
            let source_duration = clip.media_secs_at(last, fps) - source_seconds;
            sources.push(Source::Transcript {
                path: item.path.clone(),
                source_seconds,
                source_duration,
                at_ms: ((first - start) as f64 / fps * 1000.0).round() as u64,
                duration_ms: ((last - first) as f64 / fps * 1000.0).round() as u64,
            });
        }
    }
    if frames {
        for fraction in [1, 3, 5] {
            let at = start + (end - start) * fraction / 6;
            let source = timeline
                .active_video_clips_at(at)
                .into_iter()
                .rev()
                .find_map(|(_, clip)| {
                    let ClipSource::Media(id) = clip.source else {
                        return None;
                    };
                    let item = project.media_pool.get(id)?;
                    if !item.meta.has_video || item.compound.is_some() {
                        return None;
                    }
                    Some(Source::Frame {
                        path: item.path.clone(),
                        source_seconds: if let Some(freeze) = clip.freeze {
                            freeze as f64 / item.meta.fps.as_f64()
                        } else {
                            clip.media_secs_at(at, fps)
                        },
                        at_ms: ((at - start) as f64 / fps * 1000.0).round() as u64,
                    })
                });
            if let Some(source) = source {
                sources.push(source);
            }
        }
    }
    if audio {
        let solo = timeline
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Audio && track.solo);
        let mut remaining = 30.0_f64;
        let mut count = 0;
        for track in timeline
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && !track.muted && (!solo || track.solo))
        {
            for clip in track
                .clips
                .iter()
                .filter(|clip| !clip.disabled && clip.freeze.is_none())
            {
                if count >= 3 || remaining <= 0.0 {
                    break;
                }
                let first = start.max(clip.timeline_start);
                let last = end.min(clip.timeline_end());
                if first >= last {
                    continue;
                }
                let ClipSource::Media(id) = clip.source else {
                    continue;
                };
                let Some(item) = project.media_pool.get(id) else {
                    continue;
                };
                if !item.meta.has_audio || item.compound.is_some() {
                    continue;
                }
                let source_seconds = clip.media_secs_at(first, fps);
                let source_duration = (clip.media_secs_at(last, fps) - source_seconds)
                    .min(10.0)
                    .min(remaining);
                if source_duration <= 0.0 {
                    continue;
                }
                remaining -= source_duration;
                count += 1;
                sources.push(Source::Audio {
                    path: item.path.clone(),
                    source_seconds,
                    source_duration,
                    at_ms: ((first - start) as f64 / fps * 1000.0).round() as u64,
                    duration_ms: (source_duration / clip.speed().as_f64() * 1000.0).round() as u64,
                    stream: clip.audio_stream_index,
                });
            }
        }
    }
    sources
}

pub fn prepare(
    job: &mut Job,
    directory: &Path,
    parent: Option<(&Job, &Path)>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    if let Some((parent, previous)) = parent {
        for attachment in &parent.attachments {
            let from = attachment_path(previous, attachment)?;
            let to = directory.join(&attachment.file);
            if !to.exists() {
                std::fs::copy(from, &to).map_err(|e| e.to_string())?;
            }
        }
        job.attachments = parent.attachments.clone();
        return Ok(());
    }
    reuse_transcripts(job, cancel)?;
    for (index, source) in job
        .request
        .media
        .iter()
        .filter(|source| !matches!(source, Source::Transcript { .. }))
        .take(6)
        .enumerate()
    {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let extracted = match source {
            Source::Transcript {..} => continue,
            Source::Frame {path,source_seconds,at_ms} => crate::ai_context::frame_png(path,*source_seconds).map(|bytes| (bytes,Attachment {file:format!("context-frame-{index}.png"),mime:"image/png".into(),at_ms:*at_ms,description:"Representative raw source video frame; project effects are not baked in".into()})),
            Source::Audio {path,source_seconds,source_duration,at_ms,duration_ms,stream} => audio_wav(path,*source_seconds,*source_duration,*stream,cancel).map(|bytes| (bytes,Attachment {file:format!("context-audio-{index}.wav"),mime:"audio/wav".into(),at_ms:*at_ms,description:format!("Raw source audio excerpt at original speed, corresponding to {duration_ms} ms of the timeline; not the final project mix")})),
        };
        match extracted {
            Ok((bytes, attachment)) => {
                std::fs::write(directory.join(&attachment.file), bytes)
                    .map_err(|e| e.to_string())?;
                job.attachments.push(attachment);
            }
            Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
            Err(error) => job.events.push(format!(
                "Optional context unavailable: {}",
                error.chars().take(300).collect::<String>()
            )),
        }
    }
    if job.request.context == "[]"
        && !job.request.media.is_empty()
        && !job.attachments.iter().any(|attachment| {
            attachment.mime == "image/png"
                || (job.request.provider == super::Provider::Gemini
                    && attachment.mime == "audio/wav")
        })
    {
        return Err("No usable timeline context. Save captions, enable source frames/audio, or use prompt-only mode.".into());
    }
    Ok(())
}

pub fn attachment_path(directory: &Path, attachment: &Attachment) -> Result<PathBuf, String> {
    if attachment.file.is_empty()
        || !attachment
            .file
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
        || attachment.file == "."
        || attachment.file == ".."
    {
        return Err("Invalid context attachment filename".into());
    }
    let path = directory.join(&attachment.file);
    let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 4 * 1024 * 1024 {
        return Err("Context attachment must be a regular file smaller than 4 MB".into());
    }
    Ok(path)
}

pub fn audio_wav(
    path: &Path,
    seconds: f64,
    duration: f64,
    stream: usize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if duration > 30.0 {
        return Err("Invalid source audio range".into());
    }
    Ok(pcm_wav(
        &mono_16k(path, seconds, duration, stream, cancel)?,
        16_000,
    ))
}

fn mono_16k(
    path: &Path,
    seconds: f64,
    duration: f64,
    stream: usize,
    cancel: &AtomicBool,
) -> Result<Vec<f32>, String> {
    if !seconds.is_finite()
        || seconds < 0.0
        || !duration.is_finite()
        || duration <= 0.0
        || duration > 600.0
    {
        return Err("Invalid source audio range".into());
    }
    let rate = 16_000u32;
    let from = (seconds * f64::from(rate)).floor() as u64;
    let length = (duration * f64::from(rate)).round() as usize;
    let mut samples = vec![0.0f32; length];
    let mut received = false;
    vv_media::decode_audio_streams_from(path, &[stream], rate, from, |_, channels, at, chunk| {
        if cancel.load(Ordering::Relaxed) {
            return std::ops::ControlFlow::Break(());
        }
        for (index, frame) in chunk.chunks_exact(usize::from(channels.max(1))).enumerate() {
            let position = at.saturating_add(index as u64);
            if position >= from.saturating_add(length as u64) {
                return std::ops::ControlFlow::Break(());
            }
            if position >= from {
                samples[(position - from) as usize] =
                    frame.iter().sum::<f32>() / f32::from(channels.max(1));
                received = true;
            }
        }
        std::ops::ControlFlow::Continue(())
    })
    .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Cancelled".into());
    }
    if !received {
        return Err("No audio samples in the selected source range".into());
    }
    Ok(samples)
}

pub fn pcm_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let size = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(samples.len() * 2 + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        bytes
            .extend_from_slice(&((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    bytes
}

pub fn active_attachments(job: &Job) -> impl Iterator<Item = &Attachment> {
    job.attachments
        .iter()
        .filter(move |_| job.attempt <= 1 && job.request.parent.is_none() && job.session.is_none())
}

pub fn inline_parts(job: &Job, directory: &Path) -> Result<Vec<serde_json::Value>, String> {
    use base64::Engine;
    let mut parts = Vec::new();
    let mut total = 0;
    for attachment in active_attachments(job).take(6) {
        if !matches!(attachment.mime.as_str(), "image/png" | "audio/wav") {
            continue;
        }
        let bytes =
            std::fs::read(attachment_path(directory, attachment)?).map_err(|e| e.to_string())?;
        total += bytes.len();
        if total > 8 * 1024 * 1024 {
            return Err("Selected context exceeds 8 MB".into());
        }
        parts.push(serde_json::json!({"text":format!("Context at {} ms: {}",attachment.at_ms,attachment.description)}));
        parts.push(serde_json::json!({"inlineData":{"mimeType":attachment.mime,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}}));
    }
    Ok(parts)
}

pub fn image_paths(job: &Job, directory: &Path) -> Result<Vec<PathBuf>, String> {
    active_attachments(job)
        .filter(|attachment| attachment.mime == "image/png")
        .take(6)
        .map(|attachment| attachment_path(directory, attachment))
        .collect()
}

pub fn subrange(sources: &[Source], start: u64, end: u64) -> Vec<Source> {
    sources
        .iter()
        .filter_map(|source| match source {
            Source::Frame {
                path,
                source_seconds,
                at_ms,
            } if *at_ms >= start && *at_ms < end => Some(Source::Frame {
                path: path.clone(),
                source_seconds: *source_seconds,
                at_ms: at_ms - start,
            }),
            Source::Transcript {
                path,
                source_seconds,
                source_duration,
                at_ms,
                duration_ms,
            } => {
                let first = (*at_ms).max(start);
                let last = at_ms.saturating_add(*duration_ms).min(end);
                if first >= last || *duration_ms == 0 {
                    return None;
                }
                let seconds_per_ms = source_duration / (*duration_ms as f64);
                Some(Source::Transcript {
                    path: path.clone(),
                    source_seconds: source_seconds + (first - at_ms) as f64 * seconds_per_ms,
                    source_duration: (last - first) as f64 * seconds_per_ms,
                    at_ms: first - start,
                    duration_ms: last - first,
                })
            }
            Source::Audio {
                path,
                source_seconds,
                source_duration,
                at_ms,
                duration_ms,
                stream,
            } => {
                let first = (*at_ms).max(start);
                let last = at_ms.saturating_add(*duration_ms).min(end);
                if first >= last || *duration_ms == 0 {
                    return None;
                }
                let seconds_per_ms = source_duration / (*duration_ms as f64);
                Some(Source::Audio {
                    path: path.clone(),
                    source_seconds: source_seconds + (first - at_ms) as f64 * seconds_per_ms,
                    source_duration: (last - first) as f64 * seconds_per_ms,
                    at_ms: first - start,
                    duration_ms: last - first,
                    stream: *stream,
                })
            }
            _ => None,
        })
        .collect()
}

fn reuse_transcripts(job: &mut Job, cancel: &Arc<AtomicBool>) -> Result<(), String> {
    if !job.request.context.trim().is_empty() && job.request.context != "[]" {
        return Ok(());
    }
    let mut cached = std::collections::HashMap::new();
    let mut words = Vec::new();
    let mut transcribed = false;
    for source in job.request.media.iter().take(64) {
        let Source::Transcript {
            path,
            source_seconds,
            source_duration,
            at_ms,
            duration_ms,
        } = source
        else {
            continue;
        };
        if *source_duration <= 0.0 {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        let cached_words = cached.entry(path.clone()).or_insert_with(|| {
            crate::ai_clips::cached_words(path, cancel)
                .ok()
                .flatten()
                .unwrap_or_default()
        });
        let local;
        let transcript = if cached_words.is_empty() {
            // Without saved captions the CLI providers would only see a few frames.
            match transcribe_excerpt(path, *source_seconds, *source_duration, cancel) {
                Ok(words) => {
                    transcribed = true;
                    local = words;
                    &local
                }
                Err(error) if cancel.load(Ordering::Relaxed) => return Err(error),
                Err(error) => {
                    job.events.push(format!(
                        "Speech transcription unavailable: {}",
                        error.chars().take(300).collect::<String>()
                    ));
                    continue;
                }
            }
        } else {
            &*cached_words
        };
        let first = source_seconds * 1000.0;
        let last = (source_seconds + source_duration) * 1000.0;
        for word in transcript
            .iter()
            .filter(|word| f64::from(word.end_ms) > first && f64::from(word.start_ms) < last)
        {
            let start = ((*at_ms as f64)
                + (f64::from(word.start_ms) - first).max(0.0) * (*duration_ms as f64)
                    / (source_duration * 1000.0))
                .round() as u64;
            let end = ((*at_ms as f64)
                + (f64::from(word.end_ms) - first).max(0.0) * (*duration_ms as f64)
                    / (source_duration * 1000.0))
                .round() as u64;
            words.push((
                start,
                end.min(at_ms.saturating_add(*duration_ms)),
                word.text.clone(),
            ));
        }
    }
    words.sort();
    words.dedup();
    if !words.is_empty() {
        let values: Vec<_> = words
            .into_iter()
            .map(
                |(start, end, text)| serde_json::json!({"start_ms":start,"end_ms":end,"text":text}),
            )
            .collect();
        let context = serde_json::to_string(&values).map_err(|e| e.to_string())?;
        if context.len() > 512_000 {
            return Err("Cached captions exceed 512 KB; choose a smaller range".into());
        }
        job.request.context = context;
        job.events.push(if transcribed {
            "Transcribed the selected speech locally".into()
        } else {
            "Reused existing Viral Clips word timestamps".into()
        });
    }
    Ok(())
}

/// Words with source-file timestamps, like the Viral Clips cache.
fn transcribe_excerpt(
    path: &Path,
    seconds: f64,
    duration: f64,
    cancel: &Arc<AtomicBool>,
) -> Result<Vec<vv_core::CaptionWord>, String> {
    let samples = mono_16k(path, seconds, duration.min(600.0), 0, cancel)?;
    let model_dir = crate::subtitles::models_dir().ok_or("Cannot locate speech models")?;
    let preferred = vv_transcribe::Model::preferred();
    let model = [
        preferred,
        vv_transcribe::Model::Turbo,
        vv_transcribe::Model::Small,
    ]
    .into_iter()
    .find(|model| vv_transcribe::model_present(&model_dir, *model))
    .unwrap_or(preferred);
    let progress = Arc::new(vv_transcribe::Progress::default());
    let model_path = vv_transcribe::ensure_model(&model_dir, model, &progress, cancel)
        .map_err(|e| format!("{e:?}"))?;
    let transcript =
        vv_transcribe::transcribe(model, &model_path, &samples, None, &progress, cancel)
            .map_err(|e| format!("{e:?}"))?;
    let offset = (seconds * 1000.0).round() as u32;
    Ok(transcript
        .words
        .into_iter()
        .map(|word| vv_core::CaptionWord {
            text: word.text,
            start_ms: word.start_ms + offset,
            end_ms: word.end_ms + offset,
        })
        .collect())
}
