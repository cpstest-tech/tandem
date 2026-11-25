//! The Curves, Mixer and Color editors, drawn inside the Multifunction block
//! in place of the library content.

use super::*;
use crate::library_ribbon::RibbonTab;

impl VenturiApp {
    pub(crate) fn open_ribbon_editor(&mut self, editor: RibbonTab) {
        self.library_ribbon.editor = Some(editor);
        self.reveal_block(workspace::Block::Library);
    }

    pub(crate) fn color_editor_shown(&self) -> bool {
        self.library_ribbon.editor == Some(RibbonTab::Color)
            && self.block_shown(workspace::Block::Library)
    }

    /// Header with the title and «←», then the editor. Its commands are
    /// applied with the Properties ones, so a gesture stays one undo step.
    pub(crate) fn show_ribbon_editor(&mut self, ui: &mut egui::Ui, editor: RibbonTab) {
        let title = match editor {
            RibbonTab::Curves => t!("keyframes.title"),
            RibbonTab::Mixer => t!("mixer.title"),
            _ => t!("color.title"),
        };
        ui.horizontal(|ui| {
            let back = ui
                .add(
                    egui::Button::new(egui::RichText::new("←").size(theme::FONT_SECTION))
                        .min_size(egui::vec2(theme::CONTROL_HEIGHT, theme::CONTROL_HEIGHT)),
                )
                .on_hover_text(t!("ribbon.editor_back"));
            if back.clicked() {
                self.library_ribbon.editor = None;
            }
            ui.label(crate::ui_kit::section_title(title));
        });
        ui.separator();
        let (video_targets, audio_targets) = self.selection_targets();
        match editor {
            RibbonTab::Curves => self.show_curves_editor(ui, &video_targets, &audio_targets),
            RibbonTab::Mixer => self.show_mixer_editor(ui),
            _ => self.show_color_editor(ui, &video_targets),
        }
    }

    fn show_curves_editor(
        &mut self,
        ui: &mut egui::Ui,
        video_targets: &[PanelTarget],
        audio_targets: &[PanelTarget],
    ) {
        let target = video_targets
            .first()
            .or(audio_targets.first())
            .map(|t| (t.timeline, t.track_index, t.clip_id));
        let editor = keyframe_editor::show_keyframe_editor(
            ui,
            &mut self.keyframe_editor,
            &self.session.project,
            target,
            self.timeline_state.playhead,
            self.zoom_link,
        );
        self.library_ribbon.editor_commands.extend(editor.commands);
        self.library_ribbon.editor_playhead = editor.playhead;
    }

    fn show_mixer_editor(&mut self, ui: &mut egui::Ui) {
        let presets_before = (
            self.settings.compressor_presets.clone(),
            self.settings.eq_presets.clone(),
        );
        let installed = vv_plugin::installed();
        let commands = mixer_panel::show_mixer(
            ui,
            &mut self.mixer_panel,
            &self.session.project,
            self.timeline_id,
            self.timeline_audio.as_ref().map(TimelineAudio::meters),
            mixer_panel::EffectPresets {
                compressor: &mut self.settings.compressor_presets,
                eq: &mut self.settings.eq_presets,
            },
            &mixer_panel::ChainView {
                installed: &installed,
                scanning: self.plugin_scan_rx.is_some(),
                known: &|effect| {
                    self.timeline_audio
                        .as_ref()
                        .and_then(|audio| audio.plugin_known(effect))
                        .unwrap_or_else(|| installed.iter().any(|i| i.class_id == effect.class_id))
                },
                latency_ms: &|key| {
                    self.timeline_audio
                        .as_ref()
                        .and_then(|audio| audio.plugin_latency_ms(key))
                },
                failed: &|key| {
                    self.timeline_audio
                        .as_ref()
                        .is_some_and(|audio| audio.plugin_failed(key))
                },
                normalize_failed: &|channel, insert| {
                    let audio = self.timeline_audio.as_ref()?;
                    let error = audio.normalize_failure(self.timeline_id?, channel, insert)?;
                    Some(error.to_owned())
                },
            },
        );
        self.library_ribbon.editor_commands.extend(commands);
        if std::mem::take(&mut self.mixer_panel.rescan_requested) {
            self.start_plugin_scan(ui.ctx(), true);
        }
        // Saved at once: a preset must not wait for the periodic save.
        if (&self.settings.compressor_presets, &self.settings.eq_presets)
            != (&presets_before.0, &presets_before.1)
        {
            self.persist_settings();
        }
    }

    fn show_color_editor(&mut self, ui: &mut egui::Ui, video_targets: &[PanelTarget]) {
        let primary = video_targets.first().copied();
        let can_balance = self.can_auto_balance(video_targets);
        self.scope_target = primary;
        let grade = primary.map(|t| {
            self.session.project.timelines[t.timeline]
                .clip(t.track_index, t.clip_id)
                .and_then(|clip| color_window::grade_info(clip, t.source_frame))
        });
        let grade_value = grade.as_ref().and_then(|g| g.as_ref()).map(|g| g.value);
        let mut response = color_window::show_color_window(
            ui,
            &mut self.settings.panels.color_scopes,
            &mut self.scope_view,
            grade,
            can_balance,
        );
        if let (Some(section), Some(value), Some(t)) = (&mut response.grade, grade_value, primary)
            && section.auto_balance
        {
            self.balance_isolated(section, &value, t);
        }
        let tl = self
            .timeline_id
            .map(|id| &self.session.project.timelines[id]);
        let mut commands = Vec::new();
        if let Some(section) = &response.grade {
            commands.extend(grade_panel::grade_commands(tl, video_targets, section));
        }
        if response.add_grade {
            commands.extend(grade_panel::add_grade_commands(tl, video_targets));
        }
        self.library_ribbon.editor_commands.extend(commands);
        if let (Some(source_frame), Some(t)) = (response.goto, primary) {
            self.library_ribbon.editor_playhead = self.session.project.timelines[t.timeline]
                .clip(t.track_index, t.clip_id)
                .map(|c| c.timeline_frame_at(source_frame));
        }
    }
}
