//! Automatic B-roll: media of the pool whose name is a word said in the
//! transcript, laid over the speech when the word is said.

use vv_core::{FrameIdx, MediaId, TrackKind};

use crate::{VenturiApp, timeline_ui};

/// How long a B-roll shot stays, at most.
const SHOT_SECS: f64 = 3.0;
/// Words shorter than this are not matched (articles, prepositions).
const MIN_WORD: usize = 4;

/// Lowercase ASCII letters and digits, accents dropped.
fn normalized(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ä' | 'ã' => 'a',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            c => c,
        })
        .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
        .collect()
}

/// Same word, allowing a different ending (plural, gender).
fn same_word(a: &str, b: &str) -> bool {
    if a.len() < MIN_WORD || b.len() < MIN_WORD {
        return false;
    }
    let common = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    common >= MIN_WORD.max(a.len().min(b.len()).saturating_sub(2))
}

/// Shots `(media, start, seconds)` for `words` `(text, start, end)` in
/// timeline seconds, from `media` `(id, file name, longest seconds)`: each
/// media once, never two shots at the same time.
pub(crate) fn plan(
    words: &[(String, f64, f64)],
    media: &[(MediaId, String, f64)],
) -> Vec<(MediaId, f64, f64)> {
    let tokens: Vec<(MediaId, Vec<String>, f64)> = media
        .iter()
        .map(|(id, name, secs)| {
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
            let words = normalized(&stem.replace(['_', '-', '.'], " "))
                .split_whitespace()
                .map(str::to_string)
                .collect();
            (*id, words, *secs)
        })
        .collect();
    let mut used = Vec::new();
    let mut shots: Vec<(MediaId, f64, f64)> = Vec::new();
    for (text, start, _) in words {
        let word = normalized(text);
        if shots.last().is_some_and(|(_, s, len)| *start < s + len) {
            continue;
        }
        let found = tokens
            .iter()
            .find(|(id, names, _)| !used.contains(id) && names.iter().any(|n| same_word(n, &word)));
        if let Some((id, _, secs)) = found {
            used.push(*id);
            shots.push((*id, *start, secs.min(SHOT_SECS)));
        }
    }
    shots
}

impl VenturiApp {
    /// Lays the B-roll of the transcript on a new video track. Returns how
    /// many shots went in.
    pub(crate) fn insert_auto_broll(&mut self) -> usize {
        let Some(model) = &self.transcript.model else {
            return 0;
        };
        let timeline_id = model.timeline;
        let words: Vec<(String, f64, f64)> = model
            .words
            .iter()
            .map(|w| (w.text.clone(), w.start, w.end))
            .collect();
        let on_timeline: Vec<MediaId> = self.session.project.timelines[timeline_id]
            .tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter_map(|c| match c.source {
                vv_core::ClipSource::Media(id) => Some(id),
                _ => None,
            })
            .collect();
        let media: Vec<(MediaId, String, f64)> = self
            .session
            .project
            .media_pool
            .iter()
            .filter(|(id, item)| {
                item.meta.has_video && item.compound.is_none() && !on_timeline.contains(id)
            })
            .map(|(id, item)| {
                let name = item
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let secs = if item.meta.is_image() {
                    SHOT_SECS
                } else {
                    item.meta.duration_frames as f64 / item.meta.fps.as_f64().max(1e-9)
                };
                (id, name, secs)
            })
            .collect();
        let shots = plan(&words, &media);
        if shots.is_empty() {
            return 0;
        }
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let group = self.session.history.begin_group();
        let track = timeline_ui::add_track(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            TrackKind::Video,
        );
        for &(media_id, start, secs) in &shots {
            let Some(meta) = self
                .session
                .project
                .media_pool
                .get(media_id)
                .map(|i| i.meta.clone())
            else {
                continue;
            };
            let drag = if meta.is_image() {
                timeline_ui::MediaDrag::image_secs(media_id, &meta, secs)
            } else {
                timeline_ui::MediaDrag::capped_secs(media_id, &meta, secs)
            };
            vv_core::edit::insert_media(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                vv_core::edit::MediaInsert {
                    media_id,
                    source_in: drag.source_in,
                    source_out: drag.source_out,
                    video: true,
                    audio: false,
                },
                (start * fps).round() as FrameIdx,
                vv_core::edit::TargetTracks {
                    video: Some(track),
                    extra_audio: None,
                },
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
        shots.len()
    }
}

#[cfg(test)]
#[path = "tests/broll.rs"]
mod tests;
