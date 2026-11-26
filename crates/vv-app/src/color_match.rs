//! «Match color»: of two selected clips, the one under the playhead gets the
//! look of the other.

use vv_core::{ClipId, ClipSource, ColorMatrix, FrameIdx, GradeParam, TrackKind};

use crate::grade_panel::GradeSectionResponse;
use crate::{PanelTarget, VenturiApp};

/// Samples per row and column of the measured frame.
const GRID: usize = 48;

/// RGB 0–1 of a grid of points of `frame`.
fn sample_rgb(frame: &vv_media::FrameYuv420) -> Vec<[f32; 3]> {
    let (w, h) = (frame.width as usize, frame.height as usize);
    let (kr, kb) = match frame.matrix {
        ColorMatrix::Bt601 => (0.299, 0.114),
        ColorMatrix::Bt709 => (0.2126, 0.0722),
        ColorMatrix::Bt2020 => (0.2627, 0.0593),
    };
    let kg = 1.0 - kr - kb;
    let mut out = Vec::with_capacity(GRID * GRID);
    for gy in 0..GRID {
        for gx in 0..GRID {
            let (x, y) = ((gx * 2 + 1) * w / (GRID * 2), (gy * 2 + 1) * h / (GRID * 2));
            let (cx, cy) = (
                (x / 2).min(frame.chroma_width as usize - 1),
                (y / 2).min(frame.chroma_height as usize - 1),
            );
            let yv = frame.y[y * w + x] as f32 / 255.0;
            let (u, v) = match &frame.chroma {
                vv_media::Chroma::Planar { u, v } => {
                    let i = cy * frame.chroma_width as usize + cx;
                    (u[i] as f32 / 255.0, v[i] as f32 / 255.0)
                }
                vv_media::Chroma::Interleaved(uv) => {
                    let i = cy * frame.chroma_width as usize * 2 + cx * 2;
                    (uv[i] as f32 / 255.0, uv[i + 1] as f32 / 255.0)
                }
            };
            let (yn, un, vn) = if frame.full_range {
                (yv, u - 0.5, v - 0.5)
            } else {
                (
                    (yv - 16.0 / 255.0) * 255.0 / 219.0,
                    (u - 128.0 / 255.0) * 255.0 / 224.0,
                    (v - 128.0 / 255.0) * 255.0 / 224.0,
                )
            };
            let r = yn + 2.0 * (1.0 - kr) * vn;
            let b = yn + 2.0 * (1.0 - kb) * un;
            let g = (yn - kr * r - kb * b) / kg;
            out.push([r, g, b].map(|c| c.clamp(0.0, 1.0)));
        }
    }
    out
}

impl VenturiApp {
    /// The middle frame of a media clip, as RGB samples.
    fn clip_samples(
        &self,
        timeline: vv_core::TimelineId,
        (track, id): (usize, ClipId),
    ) -> Option<Vec<[f32; 3]>> {
        let clip = self.session.project.timelines[timeline].clip(track, id)?;
        let ClipSource::Media(media) = clip.source else {
            return None;
        };
        let item = self.session.project.media_pool.get(media)?;
        if !item.meta.has_video || item.compound.is_some() {
            return None;
        }
        let frame = clip.picture_frame_at(clip.timeline_start + clip.timeline_len / 2);
        let mut decoder = if item.meta.is_image() {
            vv_media::Decoder::open_image(&item.path).ok()?
        } else {
            vv_media::Decoder::open(&item.path).ok()?
        };
        let fps = decoder.fps().as_f64().max(1e-9);
        decoder.seek_to_time(frame as f64 / fps).ok()?;
        let mut last = None;
        while let Ok(Some((idx, picture))) = decoder.next_frame() {
            last = Some(picture);
            if idx >= frame || item.meta.is_image() {
                break;
            }
        }
        Some(sample_rgb(last?.as_ref()))
    }

    pub(crate) fn match_color_of_selection(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let playhead = self.timeline_state.playhead;
        let mut clips: Vec<(usize, ClipId, FrameIdx, bool)> = self
            .timeline_state
            .selected
            .iter()
            .filter(|(t, _)| tl.tracks[*t].kind == TrackKind::Video)
            .filter_map(|&(t, id)| {
                let c = tl.clip(t, id)?;
                Some((t, id, c.timeline_start, c.contains(playhead)))
            })
            .collect();
        if clips.len() != 2 {
            self.toast = Some((
                t!("color_match.pick_two").into_owned(),
                std::time::Instant::now(),
            ));
            return;
        }
        // The target is the one under the playhead, else the later one.
        clips.sort_by_key(|&(_, _, start, under)| (under, start));
        let (rt, rid, _, _) = clips[0];
        let (tt, tid, _, _) = clips[1];
        let grade_of = |track: usize, id: ClipId| {
            tl.clip(track, id)
                .and_then(|c| {
                    let f = c
                        .effects
                        .filters
                        .iter()
                        .find(|f| f.kind.has_grade() && f.enabled)?;
                    Some(
                        f.grade
                            .value_at(c.source_frame_at(c.timeline_start + c.timeline_len / 2)),
                    )
                })
                .unwrap_or(vv_core::GradeValue::NEUTRAL)
        };
        let (reference_grade, target_grade) = (grade_of(rt, rid), grade_of(tt, tid));
        let target_start = tl.clip(tt, tid).map(|c| c.timeline_start).unwrap_or(0);
        let (Some(reference), Some(target)) = (
            self.clip_samples(timeline_id, (rt, rid)),
            self.clip_samples(timeline_id, (tt, tid)),
        ) else {
            self.toast = Some((
                t!("color_match.unreadable").into_owned(),
                std::time::Instant::now(),
            ));
            return;
        };
        let reference: Vec<[f32; 3]> = reference
            .iter()
            .map(|p| vv_core::apply_grade(*p, &reference_grade))
            .collect();
        let matched = vv_core::match_color(&target_grade, &target, &reference);
        let mut section = GradeSectionResponse::default();
        for p in GradeParam::ALL {
            if matched.get(p) != target_grade.get(p) {
                section.set(p, matched.get(p));
            }
        }
        let clip = &self.session.project.timelines[timeline_id].clip(tt, tid);
        let source_frame = clip.map_or(0, |c| {
            c.source_frame_at(c.timeline_start + c.timeline_len / 2)
        });
        let targets = [PanelTarget {
            timeline: timeline_id,
            track_index: tt,
            clip_id: tid,
            source_frame,
            timeline_start: target_start,
            is_solid_color: false,
            is_text: false,
        }];
        let tl = self
            .timeline_id
            .map(|id| &self.session.project.timelines[id]);
        let commands = crate::adjust_tab::commands_for(tl, &targets, &section, None);
        if !commands.is_empty() {
            self.session.history.do_command(
                &mut self.session.project,
                Box::new(vv_core::CompositeCommand::new(
                    vv_core::CommandLabel::Filters,
                    commands,
                )),
            );
        }
        self.toast = Some((
            t!("color_match.done").into_owned(),
            std::time::Instant::now(),
        ));
    }
}

#[cfg(test)]
#[path = "tests/color_match.rs"]
mod tests;
