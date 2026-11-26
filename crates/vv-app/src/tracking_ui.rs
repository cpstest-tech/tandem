//! «Follow an object»: an overlay placed over something in the video below
//! gets position keyframes that keep it there.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use vv_core::stabilize::{ANALYSIS_WIDTH, Luma};
use vv_core::{ClipId, ClipSource, FrameIdx, Interpolation, TimelineId, TransformParam};

use crate::properties_panel::section_header;
use crate::{PanelTarget, VenturiApp};

/// Overlay keyframes every this many frames: smooth enough, not thousands.
const KEY_STEP: FrameIdx = 2;

/// The tracked point at each media frame.
type Track = Vec<(FrameIdx, [f32; 2])>;

struct Job {
    overlay: (TimelineId, usize, ClipId),
    under: (usize, ClipId),
    /// Timeline frames tracked, from the playhead.
    frames: std::ops::Range<FrameIdx>,
    done: Arc<AtomicUsize>,
    total: usize,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Track, String>>,
    /// Analysis pixel per media pixel.
    scale: f32,
}

#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
}

/// Media pixel of the picture fit into `timeline` at the timeline point
/// `p` (pixels from the centre, Y up), undoing the clip's zoom and position.
fn media_point(
    p: [f32; 2],
    t: &vv_core::Transform,
    media: (u32, u32),
    timeline: (u32, u32),
) -> [f32; 2] {
    let s = fit_scale(media, timeline);
    let q = [
        (p[0] - t.position[0]) / t.zoom[0].max(1e-3),
        (p[1] - t.position[1]) / t.zoom[1].max(1e-3),
    ];
    [
        q[0] / s + media.0 as f32 / 2.0,
        -q[1] / s + media.1 as f32 / 2.0,
    ]
}

fn timeline_point(
    m: [f32; 2],
    t: &vv_core::Transform,
    media: (u32, u32),
    timeline: (u32, u32),
) -> [f32; 2] {
    let s = fit_scale(media, timeline);
    let q = [
        (m[0] - media.0 as f32 / 2.0) * s,
        -(m[1] - media.1 as f32 / 2.0) * s,
    ];
    [
        q[0] * t.zoom[0] + t.position[0],
        q[1] * t.zoom[1] + t.position[1],
    ]
}

fn fit_scale(media: (u32, u32), timeline: (u32, u32)) -> f32 {
    (timeline.0 as f32 / media.0.max(1) as f32).min(timeline.1 as f32 / media.1.max(1) as f32)
}

/// The tracked point (analysis pixels) at each media frame of `frames`.
fn track(
    path: PathBuf,
    frames: std::ops::Range<FrameIdx>,
    start: [f32; 2],
    done: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Track, String> {
    let mut decoder = vv_media::Decoder::open(&path).map_err(|e| e.to_string())?;
    let fps = decoder.fps().as_f64().max(1e-9);
    decoder
        .seek_to_time(frames.start as f64 / fps)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut previous: Option<Luma> = None;
    let mut at = start;
    while let Some((idx, frame)) = decoder.next_frame().map_err(|e| e.to_string())? {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        if idx < frames.start {
            continue;
        }
        if idx >= frames.end {
            break;
        }
        let (w, h) = (frame.width as usize, frame.height as usize);
        let luma = Luma::downscaled(&frame.y, w, h, w);
        if let Some(prev) = &previous {
            at = vv_core::tracking::follow(prev, &luma, at);
        }
        out.push((idx, at));
        previous = Some(luma);
        done.fetch_add(1, Ordering::Relaxed);
    }
    Ok(out)
}

impl VenturiApp {
    /// The media clip below the overlay at `frame`: the topmost one on a
    /// lower video track.
    fn clip_under(
        &self,
        timeline: TimelineId,
        overlay_track: usize,
        frame: FrameIdx,
    ) -> Option<(usize, ClipId)> {
        let tl = &self.session.project.timelines[timeline];
        tl.visible_video_tracks()
            .rev()
            .skip_while(|(i, _)| *i != overlay_track)
            .skip(1)
            .find_map(|(i, t)| {
                let c = t.clips.iter().find(|c| c.contains(frame) && !c.disabled)?;
                let ClipSource::Media(media) = c.source else {
                    return None;
                };
                let item = self.session.project.media_pool.get(media)?;
                (item.meta.has_video && !item.meta.is_image() && item.compound.is_none())
                    .then_some((i, c.id))
            })
    }

    pub(crate) fn show_tracking_section(&mut self, ui: &mut egui::Ui, targets: &[PanelTarget]) {
        self.poll_tracking(ui.ctx());
        let Some(target) = targets.first().copied() else {
            return;
        };
        let playhead = self.timeline_state.playhead;
        let tl = &self.session.project.timelines[target.timeline];
        let Some(overlay) = tl.clip(target.track_index, target.clip_id) else {
            return;
        };
        let Some(under) = self.clip_under(target.timeline, target.track_index, playhead) else {
            return;
        };
        if !overlay.contains(playhead) {
            return;
        }
        ui.add_space(6.0);
        let (open, _) = section_header(ui, &t!("tracking.title"), false);
        if !open {
            return;
        }
        ui.small(t!("tracking.hint"));
        let key = (target.timeline, target.track_index, target.clip_id);
        if let Some(job) = self.tracking.job.as_ref().filter(|j| j.overlay == key) {
            let share = job.done.load(Ordering::Relaxed) as f32 / job.total.max(1) as f32;
            ui.horizontal(|ui| {
                ui.add(egui::ProgressBar::new(share).desired_width(ui.available_width() - 80.0));
                if ui.button(t!("reframe.cancel")).clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            });
            return;
        }
        if ui.button(t!("tracking.start")).clicked() {
            self.start_tracking(target, under, playhead);
        }
    }

    fn start_tracking(&mut self, target: PanelTarget, under: (usize, ClipId), from: FrameIdx) {
        let tl = &self.session.project.timelines[target.timeline];
        let (Some(overlay), Some(under_clip)) = (
            tl.clip(target.track_index, target.clip_id),
            tl.clip(under.0, under.1),
        ) else {
            return;
        };
        let ClipSource::Media(media) = under_clip.source else {
            return;
        };
        let Some(item) = self.session.project.media_pool.get(media) else {
            return;
        };
        let size = (item.meta.width, item.meta.height);
        let end = overlay.timeline_end().min(under_clip.timeline_end());
        let first = under_clip.picture_frame_at(from);
        let last = under_clip.picture_frame_at(end - 1);
        if last <= first {
            return;
        }
        let position = overlay
            .effects
            .transform
            .value_at(overlay.source_frame_at(from))
            .position;
        let under_t = under_clip
            .effects
            .transform
            .value_at(under_clip.source_frame_at(from));
        let m = media_point(position, &under_t, size, tl.resolution);
        let scale = 1.0 / (size.0 as usize / ANALYSIS_WIDTH).max(1) as f32;
        let start = [m[0] * scale, m[1] * scale];
        let path = item.path.clone();
        let done = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (job_done, job_cancel) = (done.clone(), cancel.clone());
        let range = first..last + 1;
        std::thread::Builder::new()
            .name("tracking".into())
            .spawn(move || {
                let _ = tx.send(track(path, range, start, &job_done, &job_cancel));
            })
            .expect("spawning the tracking thread");
        self.tracking.job = Some(Job {
            overlay: (target.timeline, target.track_index, target.clip_id),
            under,
            frames: from..end,
            done,
            total: (last - first + 1) as usize,
            cancel,
            rx,
            scale,
        });
    }

    pub(crate) fn poll_tracking(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.tracking.job else {
            return;
        };
        let result = match job.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("worker stopped".into()),
        };
        let Some(job) = self.tracking.job.take() else {
            return;
        };
        let points = match result {
            Ok(points) => points,
            Err(e) => {
                if e != "cancelled" {
                    self.toast = Some((e, std::time::Instant::now()));
                }
                return;
            }
        };
        let (timeline, track, id) = job.overlay;
        let Some(tl) = self.session.project.timelines.get(timeline) else {
            return;
        };
        let (Some(overlay), Some(under)) = (tl.clip(track, id), tl.clip(job.under.0, job.under.1))
        else {
            return;
        };
        let ClipSource::Media(media) = under.source else {
            return;
        };
        let Some(size) = self
            .session
            .project
            .media_pool
            .get(media)
            .map(|i| (i.meta.width, i.meta.height))
        else {
            return;
        };
        let at_media = |frame: FrameIdx| {
            points
                .iter()
                .find(|(f, _)| *f == frame)
                .or_else(|| points.last())
                .map(|(_, p)| [p[0] / job.scale, p[1] / job.scale])
        };
        let to_timeline = |t: FrameIdx| -> Option<[f32; 2]> {
            let m = at_media(under.picture_frame_at(t))?;
            let ut = under.effects.transform.value_at(under.source_frame_at(t));
            Some(timeline_point(m, &ut, size, tl.resolution))
        };
        let (Some(origin), first) = (to_timeline(job.frames.start), job.frames.start) else {
            return;
        };
        let base = overlay
            .effects
            .transform
            .value_at(overlay.source_frame_at(first))
            .position;
        let mut transform = overlay.effects.transform.clone();
        let mut t = job.frames.start;
        while t < job.frames.end {
            if let Some(p) = to_timeline(t) {
                let frame = overlay.source_frame_at(t);
                let x = base[0] + p[0] - origin[0];
                let y = base[1] + p[1] - origin[1];
                transform.track_mut(TransformParam::PositionX).upsert(
                    frame,
                    x,
                    Interpolation::Linear,
                );
                transform.track_mut(TransformParam::PositionY).upsert(
                    frame,
                    y,
                    Interpolation::Linear,
                );
            }
            t += KEY_STEP;
        }
        let auto = overlay.auto_keyframes.clone();
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(
                vv_core::auto_zoom::SetAutoZoom::new(timeline, track, id, transform, auto)
                    .with_label(vv_core::CommandLabel::TrackMotion),
            ),
        );
    }
}

#[cfg(test)]
#[path = "tests/tracking_ui.rs"]
mod tests;
