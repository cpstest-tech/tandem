use super::*;
use crate::ai_sequence::material::{self, Slot};

/// Who gets the file once the (threaded) file dialog returns.
pub(super) enum PickTarget {
    Generate,
    Suggestion {
        index: usize,
        slot: usize,
    },
    Slot {
        job: Box<Job>,
        target: PanelTarget,
        media: MediaId,
        slot: String,
    },
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl VenturiApp {
    pub(super) fn pick_material(&mut self, target: PickTarget) {
        let (tx, rx) = std::sync::mpsc::channel();
        let filter = t!("ai_sequence.material_filter").into_owned();
        std::thread::spawn(move || {
            let extensions: Vec<_> = vv_media::IMAGE_EXTENSIONS
                .iter()
                .chain(vv_media::VIDEO_EXTENSIONS)
                .copied()
                .collect();
            let _ = tx.send(
                rfd::FileDialog::new()
                    .add_filter(filter, &extensions)
                    .pick_file(),
            );
        });
        self.ai_sequences.material_pick = Some((target, rx));
    }

    pub(super) fn poll_material_pick(&mut self) {
        let Some((_, rx)) = &self.ai_sequences.material_pick else {
            return;
        };
        let path = match rx.try_recv() {
            Ok(path) => path,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let Some((target, _)) = self.ai_sequences.material_pick.take() else {
            return;
        };
        let Some(path) = path else {
            return;
        };
        match target {
            PickTarget::Generate => {
                if self.ai_sequences.material.len() < material::MAX_SLOTS {
                    self.ai_sequences.material.push((path, String::new()));
                }
            }
            PickTarget::Suggestion { index, slot } => {
                if let Some(slot) = self
                    .ai_sequences
                    .suggestions
                    .get_mut(index)
                    .and_then(|suggestion| suggestion.material.get_mut(slot))
                {
                    slot.path = Some(path);
                }
            }
            PickTarget::Slot {
                job,
                target,
                media,
                slot,
            } => {
                self.start_ai_material(&job, target, media, &slot, Some(path));
            }
        }
    }

    /// Material given before generation; the request turns it into filled spaces.
    pub(super) fn material_request(&self) -> Vec<Slot> {
        self.ai_sequences
            .material
            .iter()
            .enumerate()
            .map(|(index, (path, note))| {
                Slot::for_file(format!("material-{}", index + 1), path.clone(), note)
            })
            .collect()
    }

    pub(super) fn show_material_choice(&mut self, ui: &mut egui::Ui) {
        let mut ask = !self.ai_sequences.no_material_requests;
        if ui
            .checkbox(&mut ask, t!("ai_sequence.material_ask"))
            .on_hover_text(t!("ai_sequence.material_ask_hint"))
            .changed()
        {
            self.ai_sequences.no_material_requests = !ask;
        }
        let mut remove = None;
        for (index, (path, note)) in self.ai_sequences.material.iter_mut().enumerate() {
            ui.push_id(("material", index), |ui| {
                ui.horizontal(|ui| {
                    ui.label(file_name(path))
                        .on_hover_text(path.display().to_string());
                    if ui
                        .small_button("✕")
                        .on_hover_text(t!("ai_sequence.material_remove"))
                        .clicked()
                    {
                        remove = Some(index);
                    }
                });
                ui.add(
                    egui::TextEdit::singleline(note)
                        .desired_width(ui.available_width())
                        .hint_text(t!("ai_sequence.material_note")),
                );
            });
        }
        if let Some(index) = remove {
            self.ai_sequences.material.remove(index);
        }
        let can_add = self.ai_sequences.material_pick.is_none()
            && self.ai_sequences.material.len() < material::MAX_SLOTS;
        if ui
            .add_enabled(
                can_add,
                egui::Button::new(t!("ai_sequence.material_add")).small(),
            )
            .on_hover_text(t!("ai_sequence.material_add_hint"))
            .clicked()
        {
            self.pick_material(PickTarget::Generate);
        }
    }

    /// The spaces of a finished infographic, in the clip properties.
    pub(super) fn show_slots(
        &mut self,
        ui: &mut egui::Ui,
        current: &Job,
        target: PanelTarget,
        media: MediaId,
        busy: bool,
    ) {
        if current.slots.is_empty() {
            return;
        }
        ui.add_space(theme::SPACE_2);
        ui.strong(t!("ai_sequence.material_title"));
        ui.weak(t!("ai_sequence.material_subtitle"));
        let enabled = !busy && self.ai_sequences.material_pick.is_none();
        let mut pick = None;
        let mut clear = None;
        for slot in &current.slots {
            ui.push_id(&slot.id, |ui| {
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .stroke(egui::Stroke::new(
                        1.0,
                        if slot.filled() {
                            theme::BORDER_SUBTLE
                        } else {
                            theme::WARNING
                        },
                    ))
                    .corner_radius(theme::RADIUS_PANEL)
                    .inner_margin(theme::SPACE_2)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.strong(&slot.label);
                        if !slot.reason.is_empty() {
                            ui.label(
                                egui::RichText::new(&slot.reason)
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            );
                        }
                        ui.horizontal_wrapped(|ui| {
                            match slot.path.as_deref().filter(|_| slot.filled()) {
                                Some(path) => {
                                    ui.weak(file_name(path));
                                    if ui
                                        .add_enabled(
                                            enabled,
                                            egui::Button::new(t!("ai_sequence.material_replace"))
                                                .small(),
                                        )
                                        .clicked()
                                    {
                                        pick = Some(slot.id.clone());
                                    }
                                    if ui
                                        .add_enabled(
                                            enabled,
                                            egui::Button::new(t!("ai_sequence.material_remove"))
                                                .small(),
                                        )
                                        .clicked()
                                    {
                                        clear = Some(slot.id.clone());
                                    }
                                }
                                None => {
                                    ui.colored_label(
                                        theme::WARNING,
                                        t!("ai_sequence.material_empty"),
                                    );
                                    if ui
                                        .add_enabled(
                                            enabled,
                                            egui::Button::new(t!("ai_sequence.material_add"))
                                                .small(),
                                        )
                                        .clicked()
                                    {
                                        pick = Some(slot.id.clone());
                                    }
                                }
                            }
                        });
                    });
            });
            ui.add_space(theme::SPACE_1);
        }
        if let Some(slot) = pick {
            self.pick_material(PickTarget::Slot {
                job: Box::new(current.clone()),
                target,
                media,
                slot,
            });
        }
        if let Some(slot) = clear {
            self.start_ai_material(current, target, media, &slot, None);
        }
    }

    /// Re-renders the clip's composition with the new file: no model call.
    fn start_ai_material(
        &mut self,
        current: &Job,
        target: PanelTarget,
        media: MediaId,
        slot: &str,
        path: Option<PathBuf>,
    ) {
        let label = current
            .slots
            .iter()
            .find(|known| known.id == slot)
            .map_or(slot, |known| known.label.as_str());
        let mut request = current.request.clone();
        request.prompt = format!(
            "{label}: {}",
            path.as_deref().map_or_else(|| "—".to_owned(), file_name)
        );
        request.material = vec![Slot {
            id: slot.to_owned(),
            path,
            ..Slot::default()
        }];
        request.render_only = true;
        request.ask_material = false;
        request.parent = Some(current.id.clone());
        request.insert = false;
        request.auto_duration = false;
        request.limits = self.settings.ai_limits.clone();
        request.timeline = Some(target.timeline.raw());
        request.anchor_clip = Some(target.clip_id.0);
        if let Some(id) = self.submit_ai_sequence(request) {
            self.ai_sequences.pending_applies.insert(
                id,
                RevisionTarget {
                    timeline: target.timeline,
                    clip: target.clip_id,
                    expected: media,
                },
            );
        }
    }

    pub(super) fn show_suggestion_material(
        ui: &mut egui::Ui,
        suggestion: &mut crate::ai_sequence::suggestions::Suggestion,
        index: usize,
        picking: bool,
    ) -> Option<PickTarget> {
        if suggestion.material.is_empty() {
            return None;
        }
        let mut pick = None;
        ui.label(
            egui::RichText::new(t!("ai_sequence.material_needed"))
                .small()
                .strong(),
        );
        for (position, slot) in suggestion.material.iter_mut().enumerate() {
            ui.push_id(("needed", position), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&slot.label).on_hover_text(&slot.reason);
                    match &slot.path {
                        Some(path) => {
                            ui.weak(file_name(path));
                            if ui
                                .small_button("✕")
                                .on_hover_text(t!("ai_sequence.material_remove"))
                                .clicked()
                            {
                                slot.path = None;
                            }
                        }
                        None => {
                            if ui
                                .add_enabled(
                                    !picking,
                                    egui::Button::new(t!("ai_sequence.material_choose")).small(),
                                )
                                .on_hover_text(t!("ai_sequence.material_later"))
                                .clicked()
                            {
                                pick = Some(PickTarget::Suggestion {
                                    index,
                                    slot: position,
                                });
                            }
                        }
                    }
                });
            });
        }
        pick
    }

    /// Infographics on the timeline whose spaces still wait for the user's files.
    pub(crate) fn ai_empty_material(&self, timeline_id: TimelineId) -> usize {
        let (Some(queue), Some(timeline)) = (
            self.session
                .path()
                .and_then(|path| self.ai_sequences.queues.get(path)),
            self.session.project.timelines.get(timeline_id),
        ) else {
            return 0;
        };
        let mut jobs = std::collections::HashSet::new();
        for clip in timeline
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .filter(|clip| !clip.disabled)
        {
            if let vv_core::ClipSource::Media(media) = clip.source
                && let Some(id) = self.session.project.ai_sequences.get(&media)
                && queue
                    .jobs
                    .iter()
                    .any(|job| &job.id == id && material::empty_count(job) > 0)
            {
                jobs.insert(id.clone());
            }
        }
        jobs.len()
    }
}
