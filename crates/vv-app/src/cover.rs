//! The cover of a timeline: a frame rendered the way the export renders it,
//! on a worker thread, and the picker that chooses it.

use std::sync::{Arc, Mutex, OnceLock, mpsc};

use vv_core::{FrameIdx, ProcessingPrecision, Project, TimelineId};

use super::*;

const PREVIEW_WIDTH: u32 = 480;

pub(crate) struct Rendered {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// One frame at a time, the newest wanted one after the one in flight.
#[derive(Default)]
pub(crate) struct CoverRender {
    texture: Option<egui::TextureHandle>,
    last: Option<Rendered>,
    shown: Option<FrameIdx>,
    pending: Option<(FrameIdx, mpsc::Receiver<Option<Rendered>>)>,
}

impl CoverRender {
    /// Asks for `frame`; idempotent while it is shown or being rendered.
    pub fn request(&mut self, project: &Project, timeline: TimelineId, frame: FrameIdx) {
        if self.shown == Some(frame) || self.pending.is_some() {
            return;
        }
        let project = project.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(render(&project, timeline, frame));
        });
        self.pending = Some((frame, rx));
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some((frame, rx)) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Ok(rendered) => {
                let frame = *frame;
                self.pending = None;
                self.shown = Some(frame);
                if let Some(image) = rendered {
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    );
                    self.texture =
                        Some(ctx.load_texture("cover", color, egui::TextureOptions::LINEAR));
                    self.last = Some(image);
                }
                ctx.request_repaint();
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
        }
    }

    pub fn texture(&self) -> Option<&egui::TextureHandle> {
        self.texture.as_ref()
    }

    pub fn is_rendering(&self) -> bool {
        self.pending.is_some()
    }

    pub fn last(&self) -> Option<&Rendered> {
        self.last.as_ref()
    }
}

fn render(project: &Project, timeline: TimelineId, frame: FrameIdx) -> Option<Rendered> {
    let size = project.timelines.get(timeline)?.resolution;
    let width = PREVIEW_WIDTH.min(size.0.max(2));
    let height = ((u64::from(width) * u64::from(size.1)) / u64::from(size.0.max(1))).max(2) as u32;
    let mut cached = COMPOSITOR.lock().unwrap_or_else(|e| e.into_inner());
    let compositor = cached_compositor(&mut cached, project.precision);
    let rgba = vv_session::export::render_frame_rgba(
        project,
        timeline,
        frame,
        (width, height),
        compositor,
    )
    .ok()?;
    Some(Rendered {
        width,
        height,
        rgba,
    })
}

/// The app's own GPU device: a headless one takes seconds to create.
static SHARED_GPU: OnceLock<(wgpu::Adapter, Arc<wgpu::Device>, Arc<wgpu::Queue>)> = OnceLock::new();
static COMPOSITOR: Mutex<Option<(ProcessingPrecision, vv_render::Compositor)>> = Mutex::new(None);

fn cached_compositor(
    cached: &mut Option<(ProcessingPrecision, vv_render::Compositor)>,
    precision: ProcessingPrecision,
) -> &vv_render::Compositor {
    if cached.as_ref().is_none_or(|(p, _)| *p != precision) {
        let compositor = match SHARED_GPU.get() {
            Some((adapter, device, queue)) => {
                vv_render::Compositor::new(adapter, device.clone(), queue.clone(), precision)
            }
            None => vv_render::Compositor::new_headless_with_precision(precision),
        };
        *cached = Some((precision, compositor));
    }
    &cached.as_ref().expect("set above").1
}

/// Shares the app's device with the cover renders and builds their
/// compositor in the background, so the export window opens with a cover.
pub(crate) fn warm_up(
    render_state: &eframe::egui_wgpu::RenderState,
    precision: ProcessingPrecision,
) {
    let _ = SHARED_GPU.set((
        render_state.adapter.clone(),
        Arc::new(render_state.device.clone()),
        Arc::new(render_state.queue.clone()),
    ));
    std::thread::spawn(move || {
        let mut cached = COMPOSITOR.lock().unwrap_or_else(|e| e.into_inner());
        cached_compositor(&mut cached, precision);
    });
}

/// Pulsing placeholder while a picture is on its way.
pub(crate) fn paint_skeleton(ui: &egui::Ui, rect: egui::Rect) {
    let time = ui.input(|i| i.time);
    let t = (0.5 + 0.5 * (time * 3.0).sin()) as f32;
    let color = theme::BG_INSET.lerp_to_gamma(theme::BG_RAISED, t);
    ui.painter().rect_filled(rect, theme::RADIUS_PANEL, color);
    ui.ctx().request_repaint();
}

/// Draws `texture` fitted into `rect` over a black card.
pub(crate) fn paint_fitted(ui: &egui::Ui, rect: egui::Rect, texture: Option<&egui::TextureHandle>) {
    ui.painter()
        .rect_filled(rect, theme::RADIUS_PANEL, egui::Color32::BLACK);
    let Some(texture) = texture else {
        return;
    };
    let size = texture.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    ui.painter().image(
        texture.id(),
        egui::Rect::from_center_size(rect.center(), size * scale),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

pub(crate) struct CoverPicker {
    frame: FrameIdx,
    render: CoverRender,
}

impl CoverPicker {
    pub fn new(frame: FrameIdx) -> Self {
        Self {
            frame,
            render: CoverRender::default(),
        }
    }
}

impl VenturiApp {
    /// The frame of the cover: the chosen one, else the first.
    pub(crate) fn cover_frame(&self, timeline: TimelineId) -> FrameIdx {
        self.session
            .project
            .view
            .covers
            .get(&timeline)
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn open_cover_picker(&mut self) {
        if let Some(timeline) = self.timeline_id {
            self.cover_picker = Some(CoverPicker::new(self.cover_frame(timeline)));
        }
    }

    pub(crate) fn show_cover_picker(&mut self, ctx: &egui::Context) {
        let (Some(picker), Some(timeline_id)) = (&mut self.cover_picker, self.timeline_id) else {
            return;
        };
        let Some(timeline) = self.session.project.timelines.get(timeline_id) else {
            self.cover_picker = None;
            return;
        };
        let last_frame = (timeline.total_frames() - 1).max(0);
        let fps = timeline.fps.as_f64();
        picker.frame = picker.frame.clamp(0, last_frame);
        picker
            .render
            .request(&self.session.project, timeline_id, picker.frame);
        picker.render.poll(ctx);
        let (mut confirm, mut cancel) = (false, false);
        let modal = egui::Modal::new(egui::Id::new("cover_picker")).show(ctx, |ui| {
            ui.set_width(420.0);
            crate::ui_kit::heading(ui, t!("cover.title"));
            ui.add_space(theme::SPACE_2);
            let preview = egui::vec2(ui.available_width(), 260.0);
            let (rect, _) = ui.allocate_exact_size(preview, egui::Sense::hover());
            match picker.render.texture() {
                Some(texture) => paint_fitted(ui, rect, Some(texture)),
                None => paint_skeleton(ui, rect),
            }
            if picker.render.is_rendering() {
                ui.put(
                    egui::Rect::from_center_size(rect.center(), egui::vec2(24.0, 24.0)),
                    egui::Spinner::new(),
                );
            }
            ui.add_space(theme::SPACE_2);
            ui.spacing_mut().slider_width = ui.available_width() - 90.0;
            let label = timeline_ui::format_timecode(picker.frame as f64 / fps, fps);
            ui.add(
                egui::Slider::new(&mut picker.frame, 0..=last_frame)
                    .show_value(false)
                    .text(label),
            );
            ui.add_space(theme::SPACE_3);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                confirm = ui.button(t!("cover.confirm")).clicked();
                cancel = ui.button(t!("common.cancel")).clicked();
            });
        });
        if cancel || modal.should_close() {
            self.cover_picker = None;
        } else if confirm {
            self.confirm_cover();
        }
    }

    fn confirm_cover(&mut self) {
        let (Some(picker), Some(timeline_id)) = (self.cover_picker.take(), self.timeline_id) else {
            return;
        };
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::SetCover::new(timeline_id, picker.frame)),
        );
        if let (Some(path), Some(image)) = (self.session.path(), picker.render.last()) {
            projects::write_custom_cover(path, image.width, image.height, &image.rgba);
        }
    }
}
