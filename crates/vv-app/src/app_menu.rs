//! Top bar (Home, menus, project name, Export) and keyboard shortcuts.

use super::*;

const DISCORD_INVITE: &str = "https://discord.gg/3v2xjXFnZv";
const TOP_BAR_TITLE_MIN_WIDTH: f32 = 760.0;

impl VenturiApp {
    pub(crate) fn handle_shortcuts(&mut self, ui: &mut egui::Ui) {
        if self.dub.open {
            return;
        }
        // Copy/paste are handled outside `ui.input`: `ctx.copy_text` takes
        // the same lock and would deadlock inside it.
        let mut clipboard_events: Vec<egui::Event> = Vec::new();
        let mut arrow_input = (None, 0.0);
        let mut set_fullscreen = None;
        // Keys typed into a text field (e.g. the title) are not shortcuts:
        // "T" would cut the clips, Backspace would delete them.
        let typing = ui.ctx().egui_wants_keyboard_input();
        // An open menu owns the keyboard: its letters jump to items.
        let menu_open = egui::Popup::is_any_open(ui.ctx());
        if let Some(timeline_id) = self.timeline_id {
            self.timeline_state
                .drop_locked(&self.session.project.timelines[timeline_id]);
        }
        let blocks_shortcuts = self
            .settings_dialog
            .as_ref()
            .is_some_and(|d| d.is_capturing())
            || self
                .preset_dialog
                .as_ref()
                .is_some_and(|d| d.is_capturing())
            || self.library_ui.confirm_delete.is_some()
            || self.export_modal.is_some()
            || self.auto_cut.modal.is_some()
            || self.cover_picker.is_some()
            || self.pending_project_switch.is_some();
        let library_tab = self.library_ui.tab == library_ui::PoolTab::Library;
        // The transcript panel owns Del and Ctrl+A after a click in it.
        let transcript_keys = self.media_pool_state.focused
            && self.transcript.focused
            && self.library_ribbon.editor.is_none()
            && self.library_ribbon.tab == library_ribbon::RibbonTab::Transcript
            && self.block_shown(Block::Library);
        let keymap = self.settings.keymap.clone();
        let menu_paste = std::mem::take(&mut self.menu_paste_pending);
        let ctx = ui.ctx().clone();
        let mut open_compound = false;
        let mut make_compound = false;
        let mut ungroup_compound = false;
        let mut exit_compound = false;
        ui.input(|i| {
            arrow_input.1 = i.time;
            // Mouse "back" button, as in a web browser.
            exit_compound = i.pointer.button_pressed(egui::PointerButton::Extra1);
            if typing || blocks_shortcuts || menu_open {
                return;
            }
            let pressed = |action| keymap.pressed(action, i);
            arrow_input.0 = match (
                keymap.down(Action::StepBackward, i),
                keymap.down(Action::StepForward, i),
            ) {
                (true, false) => Some(-1),
                (false, true) => Some(1),
                _ => None,
            };
            if pressed(Action::Delete) {
                // The panel that got the last click decides who deletes:
                // keyframe editor, media pool or timeline.
                if self.keyframe_editor.owns_delete() {
                    self.delete_selected_keyframes();
                } else if transcript_keys {
                    self.transcript.delete_requested = true;
                } else if self.media_pool_state.focused
                    && self.library_ribbon.tab == library_ribbon::RibbonTab::Styles
                    && self.library_ribbon.editor.is_none()
                {
                    // Nothing to delete with a key there: the pool is not shown.
                } else if self.media_pool_state.focused && library_tab {
                    self.delete_selected_library_assets();
                } else if self.media_pool_state.focused {
                    self.delete_selected_media();
                } else {
                    self.delete_selected();
                }
            }
            if pressed(Action::RippleDelete) {
                self.ripple_delete_selected();
            }
            if pressed(Action::Split) {
                self.split_at_playhead();
            }
            for (action, tool) in [
                (Action::SelectTool, timeline_ui::TimelineTool::Select),
                (Action::BladeTool, timeline_ui::TimelineTool::Blade),
                (
                    Action::SelectLeftTool,
                    timeline_ui::TimelineTool::SelectLeft,
                ),
                (
                    Action::SelectRightTool,
                    timeline_ui::TimelineTool::SelectRight,
                ),
                (Action::SlipTool, timeline_ui::TimelineTool::Slip),
            ] {
                if pressed(action) {
                    self.set_timeline_tool(tool);
                }
            }
            for (action, command) in [
                (Action::DeleteLeft, timeline_ui::TimelineCommand::DeleteLeft),
                (
                    Action::DeleteRight,
                    timeline_ui::TimelineCommand::DeleteRight,
                ),
                (Action::Duplicate, timeline_ui::TimelineCommand::Duplicate),
                (
                    Action::ExtractAudio,
                    timeline_ui::TimelineCommand::ExtractAudio,
                ),
                (Action::GroupClips, timeline_ui::TimelineCommand::Group),
            ] {
                if pressed(action) {
                    self.run_timeline_command(&ctx, command);
                }
            }
            for (action, mode) in [
                (Action::InsertEdit, vv_core::edit::EditMode::Insert),
                (Action::OverwriteEdit, vv_core::edit::EditMode::Overwrite),
                (Action::AppendEdit, vv_core::edit::EditMode::Append),
            ] {
                if pressed(action) {
                    self.three_point_edit(mode);
                }
            }
            for (action, delta) in [
                (Action::NudgeLeft, -1),
                (Action::NudgeRight, 1),
                (Action::NudgeLeft10, -10),
                (Action::NudgeRight10, 10),
            ] {
                if pressed(action) {
                    self.nudge_selected(delta);
                }
            }
            if pressed(Action::MatchFrame) {
                self.match_frame();
            }
            if pressed(Action::CloseGaps) {
                self.close_all_gaps();
            }
            if pressed(Action::ToggleMagnet) {
                self.timeline_state.magnet = !self.timeline_state.magnet;
            }
            if pressed(Action::ToggleSnapping) {
                self.snapping_enabled = !self.snapping_enabled;
            }
            if pressed(Action::TogglePreviewAxis) {
                self.timeline_state.preview_axis = !self.timeline_state.preview_axis;
            }
            if pressed(Action::ToggleLinkage) {
                self.timeline_state.linkage = !self.timeline_state.linkage;
            }
            if pressed(Action::FitTimeline) {
                self.timeline_state.fit_requested = true;
            }
            if pressed(Action::GoToStart) {
                self.go_to_start();
            }
            if pressed(Action::GoToEnd) {
                self.go_to_end();
            }
            if pressed(Action::PreviousEditPoint) {
                self.jump_to_edit_point(false);
            }
            if pressed(Action::NextEditPoint) {
                self.jump_to_edit_point(true);
            }
            if pressed(Action::StepBackward10) {
                self.step_playhead_by_ten(false);
            }
            if pressed(Action::StepForward10) {
                self.step_playhead_by_ten(true);
            }
            if pressed(Action::ShuttleBack) {
                self.shuttle_back();
            }
            if pressed(Action::ShuttlePause) {
                self.shuttle_pause();
            }
            if pressed(Action::ShuttleForward) {
                self.shuttle_forward();
            }
            // Enter also ends the mask being drawn in the viewer.
            open_compound = pressed(Action::OpenCompound) && !self.mask_overlay.drawing;
            exit_compound |= pressed(Action::ExitCompound);
            if pressed(Action::MakeCompound) {
                make_compound = true;
            }
            if pressed(Action::UngroupCompound) {
                ungroup_compound = true;
            }
            if pressed(Action::ToggleDisabled) {
                self.toggle_disabled_selected();
            }
            if pressed(Action::RetimeControls) {
                self.toggle_retime_controls();
            }
            if pressed(Action::Undo) {
                self.undo();
            }
            if pressed(Action::Redo) {
                self.redo();
            }
            if pressed(Action::TogglePlayback) {
                self.toggle_playback();
            }
            if pressed(Action::MarkIn) {
                self.mark_at_playhead(true);
            }
            if pressed(Action::MarkOut) {
                self.mark_at_playhead(false);
            }
            if pressed(Action::ClearIn) {
                self.clear_in_point();
            }
            if pressed(Action::ClearOut) {
                self.clear_out_point();
            }
            if pressed(Action::ClearInOut) {
                self.clear_in_and_out();
            }
            if pressed(Action::FastPlayback) {
                self.handle_fast_playback_key();
            }
            if pressed(Action::SelectAll) {
                // Same rule as Del: the panel with the last click
                // decides what Ctrl+A selects.
                if transcript_keys {
                    self.transcript.select_all_requested = true;
                } else if self.media_pool_state.focused && library_tab {
                    self.select_all_library_assets();
                } else if self.media_pool_state.focused {
                    self.select_all_media();
                } else {
                    self.select_all_clips();
                }
            }
            if pressed(Action::Rename) && self.media_pool_state.focused {
                if library_tab {
                    self.rename_selected_library_item();
                } else {
                    self.rename_selected_pool_item();
                }
            }
            if pressed(Action::SelectFromPlayhead) {
                self.select_clips_from_playhead();
            }
            if pressed(Action::ImportMedia) {
                self.import_media_dialog();
            }
            if pressed(Action::SaveProject) {
                self.save_project();
            }
            if pressed(Action::SaveProjectAs) {
                self.save_project_as();
            }
            if pressed(Action::NewProject) {
                self.request_project_switch(ProjectSwitch::New);
            }
            if pressed(Action::OpenProject) {
                self.request_project_switch(ProjectSwitch::Open);
            }
            if pressed(Action::Export) {
                self.start_export();
            }
            if pressed(Action::OpenSettings) {
                self.open_settings(settings_dialog::Section::General);
            }
            // Only collected: handled outside here, see above.
            let pasted_text = i
                .events
                .iter()
                .find_map(|e| match e {
                    egui::Event::Paste(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            for (action, event) in [
                (Action::Copy, egui::Event::Copy),
                (Action::Cut, egui::Event::Cut),
                (Action::Paste, egui::Event::Paste(pasted_text)),
            ] {
                let from_menu = menu_paste && matches!(event, egui::Event::Paste(_));
                if pressed(action) || from_menu {
                    clipboard_events.push(event);
                }
            }
            if pressed(Action::PasteAttributes) {
                self.open_paste_attributes_dialog();
            }
            // An action bound later to the same keys wins over the preset.
            if !Action::ALL.into_iter().any(pressed)
                && let Some(id) = self.library.preset_pressed(i)
            {
                self.apply_preset_to_selection(&id);
            }
            if pressed(Action::ZoomIn) {
                self.timeline_state.zoom_in();
            }
            if pressed(Action::ZoomOut) {
                self.timeline_state.zoom_out();
            }
            if pressed(Action::AddMarker) {
                self.add_marker_at_playhead();
            }
            if pressed(Action::ViewerZoomFit) {
                self.viewer_zoom.fit();
            }
            if pressed(Action::ViewerZoomActual)
                && let Some((area, frame_px)) = self.viewer_geometry
            {
                self.viewer_zoom
                    .set_scale(1.0, area, frame_px, i.pixels_per_point);
            }
            if pressed(Action::FullscreenViewer) {
                set_fullscreen = Some(!self.viewer_fullscreen);
            }
            if self.viewer_fullscreen && i.key_pressed(egui::Key::Escape) {
                set_fullscreen = Some(false);
            }
        });
        // Outside `ui.input`: `send_viewport_cmd` takes the same lock again.
        if let Some(on) = set_fullscreen.take() {
            self.viewer_fullscreen = on;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
        }
        // Served by the timeline panel, where the double click and the
        // breadcrumb are: there playback follows the jumping playhead. Plain
        // assignments, so a request not served this frame does not fire later.
        let pool_focused = self.media_pool_state.focused;
        if open_compound && pool_focused {
            if library_tab {
                self.add_selected_library_assets_to_project();
            } else {
                self.open_selected_pool_compound();
            }
        }
        self.timeline_state.open_selected_compound = open_compound && !pool_focused;
        if make_compound && !pool_focused {
            self.make_compound_from_selection();
        }
        if ungroup_compound && !pool_focused {
            self.ungroup_selected_compound();
        }
        self.exit_compound_requested = exit_compound;
        self.handle_clipboard_events(ui, &clipboard_events);
        if self.step_playhead_with_arrows(arrow_input.0, arrow_input.1) {
            ui.ctx().request_repaint();
        }
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        if self.drive_reverse_shuttle(dt) {
            ui.ctx().request_repaint();
        }
    }

    /// Requests a normal close of the application's root viewport via `ctx`.
    ///
    /// The window close lifecycle remains responsible for saving session state
    /// and handling any unsaved changes.
    fn request_exit(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Draws the File menu's Exit item and requests a normal viewport close.
    fn show_exit_menu_item(&self, ui: &mut egui::Ui) -> egui::Response {
        let response = ui.button(t!("menu.exit"));
        if response.clicked() {
            self.request_exit(ui.ctx());
            ui.close();
        }
        response
    }

    /// Displays the top bar in `ui` and dispatches the chosen actions.
    pub(crate) fn show_top_bar(&mut self, ui: &mut egui::Ui) {
        let keymap = self.settings.keymap.clone();
        let mut set_fullscreen = None;
        let mut go_home = false;
        egui::Panel::top("menu_bar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_WINDOW)
                    .inner_margin(egui::Margin::symmetric(theme::SPACE_2 as i8, 6)),
            )
            .show(ui, |ui| {
                let row = ui
                    .horizontal(|ui| {
                        go_home = ui.button("🏠").on_hover_text(t!("top_bar.home")).clicked();
                        self.show_menus(ui, &keymap, &mut set_fullscreen);
                    })
                    .response
                    .rect;
                let row = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), row.y_range());
                // On a narrow window the title would cover the menus.
                if row.width() >= TOP_BAR_TITLE_MIN_WIDTH {
                    self.show_top_bar_center(ui, row);
                }
                let mut right = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(row)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                let export = egui::Button::new(
                    egui::RichText::new(t!("top_bar.export"))
                        .color(theme::ON_ACCENT)
                        .size(theme::FONT_BODY),
                )
                .fill(theme::ACCENT)
                .corner_radius(theme::RADIUS_CONTROL);
                if right.add(export).clicked() {
                    self.open_export_modal();
                }
            });

        if go_home {
            self.request_project_switch(ProjectSwitch::Home);
        }
        if let Some(true) = set_fullscreen.take() {
            self.viewer_fullscreen = true;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
    }

    /// The project name, clickable to rename it, and whether it is saved;
    /// centred on the bar.
    fn show_top_bar_center(&mut self, ui: &mut egui::Ui, row: egui::Rect) {
        let width = self.top_bar_center_width.max(1.0);
        let rect = egui::Rect::from_center_size(row.center(), egui::vec2(width, row.height()));
        let mut center = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        // Measured to centre it next frame: wrapping would shrink it for good.
        center.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        self.show_project_title(&mut center);
        let drawn = center.min_rect().width();
        if (drawn - self.top_bar_center_width).abs() > 0.5 {
            self.top_bar_center_width = drawn;
            ui.ctx().request_repaint();
        }
    }

    fn show_project_title(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.session.path().map(Path::to_path_buf) else {
            ui.strong(self.project_label());
            return;
        };
        if let Some(name) = self.project_name_edit.as_mut() {
            let edit = ui.add(egui::TextEdit::singleline(name).desired_width(220.0));
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            if edit.lost_focus() {
                let cancelled = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let name = self.project_name_edit.take().unwrap_or_default();
                if !cancelled {
                    self.rename_open_project(&name);
                }
            }
        } else {
            let name = projects::project_name(&path);
            let title = ui
                .add(egui::Button::new(egui::RichText::new(&name).strong()).frame(false))
                .on_hover_text(t!("top_bar.rename_hint"));
            if title.clicked() {
                self.project_name_edit = Some(name);
            }
        }
        let (text, color, detail) = match self.save_status() {
            autosave::SaveStatus::Saved => {
                (t!("top_bar.saved"), ui.visuals().weak_text_color(), None)
            }
            autosave::SaveStatus::Saving => {
                (t!("top_bar.saving"), ui.visuals().weak_text_color(), None)
            }
            autosave::SaveStatus::Failed(error) => {
                (t!("top_bar.save_failed"), theme::ERROR, Some(error))
            }
        };
        let status = ui.label(egui::RichText::new(text).small().color(color));
        if let Some(detail) = detail {
            status.on_hover_text(detail);
        }
    }

    fn show_menus(
        &mut self,
        ui: &mut egui::Ui,
        keymap: &settings::Keymap,
        set_fullscreen: &mut Option<bool>,
    ) {
        egui::MenuBar::new().ui(ui, |ui| {
            let mut bar_menus = Vec::new();
            let export_disabled =
                self.timeline_id.is_none() || self.export.is_some() || self.export_modal.is_some();

            bar_menus.push(
                ui.menu_button(t!("menu.file"), |ui| {
                    if ui
                        .button(keymap.menu_label(&t!("menu.new_project"), Action::NewProject))
                        .clicked()
                    {
                        self.request_project_switch(ProjectSwitch::New);
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.open_project"), Action::OpenProject))
                        .clicked()
                    {
                        self.request_project_switch(ProjectSwitch::Open);
                        ui.close();
                    }
                    self.recent_projects_menu(ui);
                    if ui
                        .button(keymap.menu_label(&t!("menu.save"), Action::SaveProject))
                        .clicked()
                    {
                        self.save_project();
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.save_as"), Action::SaveProjectAs))
                        .clicked()
                    {
                        self.save_project_as();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(keymap.menu_label(&t!("menu.import_media"), Action::ImportMedia))
                        .clicked()
                    {
                        self.import_media_dialog();
                        ui.close();
                    }
                    if ui
                        .button(t!("menu.import_otio"))
                        .on_hover_text(t!("menu.import_otio_hint"))
                        .clicked()
                    {
                        self.import_otio_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            !export_disabled,
                            egui::Button::new(
                                keymap.menu_label(&t!("menu.export"), Action::Export),
                            ),
                        )
                        .on_hover_text(t!("menu.export_hint"))
                        .clicked()
                    {
                        self.start_export();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.timeline_id.is_some(),
                            egui::Button::new(t!("menu.export_otio")),
                        )
                        .on_hover_text(t!("menu.export_otio_hint"))
                        .clicked()
                    {
                        self.export_otio_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(keymap.menu_label(&t!("menu.settings"), Action::OpenSettings))
                        .clicked()
                    {
                        self.open_settings(settings_dialog::Section::General);
                        ui.close();
                    }
                    ui.separator();
                    self.show_exit_menu_item(ui);
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.edit"), |ui| {
                    if ui
                        .button(keymap.menu_label(&t!("menu.undo"), Action::Undo))
                        .clicked()
                    {
                        self.undo();
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.redo"), Action::Redo))
                        .clicked()
                    {
                        self.redo();
                        ui.close();
                    }
                    self.undo_history_menu(ui);
                    ui.separator();
                    if ui
                        .add_enabled(
                            !self.timeline_state.selected.is_empty(),
                            egui::Button::new(keymap.menu_label(&t!("menu.copy"), Action::Copy)),
                        )
                        .clicked()
                    {
                        // Also writes the placeholder to the system clipboard, see
                        // `handle_clipboard_events`.
                        self.handle_clipboard_events(ui, &[egui::Event::Copy]);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.timeline_state.selected.is_empty(),
                            egui::Button::new(keymap.menu_label(&t!("menu.cut"), Action::Cut)),
                        )
                        .clicked()
                    {
                        self.handle_clipboard_events(ui, &[egui::Event::Cut]);
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.paste"), Action::Paste))
                        .on_hover_text(t!("menu.paste_hint"))
                        .clicked()
                    {
                        // Another instance may have copied: only the
                        // system clipboard knows.
                        self.menu_paste_pending = true;
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                        ui.ctx().request_repaint();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.timeline_state.clipboard.is_empty()
                                && !self.timeline_state.selected.is_empty(),
                            egui::Button::new(
                                keymap.menu_label(
                                    &t!("menu.paste_attributes"),
                                    Action::PasteAttributes,
                                ),
                            ),
                        )
                        .on_hover_text(t!("menu.paste_attributes_hint"))
                        .clicked()
                    {
                        self.open_paste_attributes_dialog();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.timeline_state.selected.is_empty(),
                            egui::Button::new(t!("timeline.save_preset")),
                        )
                        .clicked()
                    {
                        let clips: Vec<_> = self.timeline_state.selected.iter().copied().collect();
                        self.open_save_preset_dialog(&clips);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .button(keymap.menu_label(&t!("menu.delete"), Action::Delete))
                        .clicked()
                    {
                        self.delete_selected();
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.ripple_delete"), Action::RippleDelete))
                        .on_hover_text(t!("menu.ripple_delete_hint"))
                        .clicked()
                    {
                        self.ripple_delete_selected();
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.split"), Action::Split))
                        .on_hover_text(t!("menu.split_hint"))
                        .clicked()
                    {
                        self.split_at_playhead();
                        ui.close();
                    }
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.project"), |ui| {
                    if ui.button(t!("menu.project_settings")).clicked() {
                        self.project_settings_open = true;
                        ui.close();
                    }
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.timeline"), |ui| {
                    // Checkboxes: they stay open on click, unlike the
                    // action buttons elsewhere in the menus.
                    ui.checkbox(
                        &mut self.selection_follows_playhead,
                        t!("menu.selection_follows_playhead"),
                    )
                    .on_hover_text(t!("menu.selection_follows_playhead_hint"));
                    ui.checkbox(&mut self.scrub_audio, t!("menu.scrub_audio"))
                        .on_hover_text(t!("menu.scrub_audio_hint"));
                    ui.separator();
                    if ui
                        .button(keymap.menu_label(&t!("menu.zoom_in"), Action::ZoomIn))
                        .clicked()
                    {
                        self.timeline_state.zoom_in();
                        ui.close();
                    }
                    if ui
                        .button(keymap.menu_label(&t!("menu.zoom_out"), Action::ZoomOut))
                        .clicked()
                    {
                        self.timeline_state.zoom_out();
                        ui.close();
                    }
                    ui.label(t!("menu.zoom_hint"));
                    ui.separator();
                    if ui
                        .button(
                            keymap.menu_label(
                                &t!("timeline.retime_controls"),
                                Action::RetimeControls,
                            ),
                        )
                        .clicked()
                    {
                        self.toggle_retime_controls();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.timeline_state.selected.is_empty(),
                            egui::Button::new(t!("timeline.remove_silences")),
                        )
                        .clicked()
                    {
                        self.timeline_state.silence_dialog_requested =
                            Some(self.timeline_state.selected.iter().copied().collect());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.timeline_id.is_some(),
                            egui::Button::new(
                                keymap.menu_label(&t!("timeline.add_marker"), Action::AddMarker),
                            ),
                        )
                        .clicked()
                    {
                        self.add_marker_at_playhead();
                        ui.close();
                    }
                    let (can_mark, marks) = self
                        .mark_target()
                        .map(|target| (target.total > 0, Some(*target.marks)))
                        .unwrap_or((false, None));
                    for (action, is_in) in [(Action::MarkIn, true), (Action::MarkOut, false)] {
                        if ui
                            .add_enabled(
                                can_mark,
                                egui::Button::new(keymap.menu_label(&action.label(), action)),
                            )
                            .clicked()
                        {
                            self.mark_at_playhead(is_in);
                            ui.close();
                        }
                    }
                    let has_in = marks.is_some_and(|m| m.mark_in.is_some());
                    let has_out = marks.is_some_and(|m| m.mark_out.is_some());
                    for (action, enabled, clear) in [
                        (
                            Action::ClearIn,
                            has_in,
                            Self::clear_in_point as fn(&mut Self),
                        ),
                        (Action::ClearOut, has_out, Self::clear_out_point),
                        (
                            Action::ClearInOut,
                            has_in || has_out,
                            Self::clear_in_and_out,
                        ),
                    ] {
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(keymap.menu_label(&action.label(), action)),
                            )
                            .clicked()
                        {
                            clear(self);
                            ui.close();
                        }
                    }
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.playback"), |ui| {
                    if ui
                        .checkbox(&mut self.settings.proxy_enabled, t!("menu.use_proxy"))
                        .on_hover_text(t!("menu.use_proxy_hint"))
                        .changed()
                    {
                        self.apply_proxy_settings();
                        self.persist_settings();
                    }
                    ui.menu_button(t!("menu.preview_resolution"), |ui| {
                        for resolution in crate::settings::PreviewResolution::ALL {
                            let label = t!(format!("menu.preview_resolution_{}", resolution.id()));
                            if ui
                                .radio(self.settings.preview_resolution == resolution, label)
                                .clicked()
                            {
                                self.settings.preview_resolution = resolution;
                                self.apply_preview_resolution();
                                self.persist_settings();
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(t!("menu.preview_resolution_hint"));
                    if ui.button(t!("menu.playback_settings")).clicked() {
                        self.open_settings(settings_dialog::Section::Playback);
                        ui.close();
                    }
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.view"), |ui| {
                    ui.checkbox(&mut self.audiometer_enabled, t!("menu.audiometer"))
                        .on_hover_text(t!("menu.audiometer_hint"));
                    ui.separator();
                    if ui
                        .button(
                            keymap.menu_label(
                                &t!("menu.fullscreen_player"),
                                Action::FullscreenViewer,
                            ),
                        )
                        .clicked()
                    {
                        *set_fullscreen = Some(true);
                        ui.close();
                    }
                })
                .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.layout"), |ui| self.layout_menu(ui))
                    .response,
            );

            bar_menus.push(
                ui.menu_button(t!("menu.help"), |ui| {
                    if ui.button(t!("menu.discord")).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(DISCORD_INVITE));
                        ui.close();
                    }
                    if ui.button(t!("update.check_menu")).clicked() {
                        self.start_update_check(ui.ctx(), true);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t!("menu.about")).clicked() {
                        self.about_open = true;
                        ui.close();
                    }
                })
                .response,
            );
            switch_bar_menu_on_hover(ui.ctx(), &bar_menus);
        });
    }

    fn layout_menu(&mut self, ui: &mut egui::Ui) {
        use crate::vertical_layout::LayoutMode;
        let current = self.settings.shorts.layout;
        for (mode, label) in [
            (LayoutMode::Desktop, t!("layout.mode_desktop")),
            (LayoutMode::Vertical, t!("layout.mode_vertical")),
        ] {
            if ui.radio(current == mode, label).clicked() {
                self.set_layout(mode);
                ui.close();
            }
        }
        if ui
            .checkbox(
                &mut self.settings.shorts.propose_layout,
                t!("layout.propose_setting"),
            )
            .changed()
        {
            self.persist_settings();
        }
        ui.separator();
        if ui.button(t!("workspace.reset")).clicked() {
            self.workspace.reset();
            ui.close();
        }
        ui.separator();
        ui.label(egui::RichText::new(t!("workspace.closed_blocks")).weak());
        let closed = self.workspace.closed_blocks();
        if closed.is_empty() {
            ui.add_enabled(false, egui::Button::new(t!("workspace.none_closed")));
        }
        for block in closed {
            if ui
                .button(t!("workspace.reopen", block = block.title()))
                .clicked()
            {
                self.workspace.reopen(block);
                ui.close();
            }
        }
        let detached = self.workspace.detached_blocks();
        if !detached.is_empty() {
            ui.separator();
            for block in detached {
                if ui
                    .button(t!("workspace.redock_block", block = block.title()))
                    .clicked()
                {
                    self.workspace.redock(block);
                    ui.close();
                }
            }
        }
    }

    pub(crate) fn show_about_dialog(&mut self, ctx: &egui::Context) {
        let icon = self.about_icon(ctx);
        egui::Window::new(t!("about.title"))
            .open(&mut self.about_open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(texture) = &icon {
                        ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(96.0, 96.0)));
                    }
                    ui.heading("Tandem video editor");
                    ui.label(t!("about.version", version = env!("VV_VERSION")));
                    ui.add_space(8.0);
                    ui.label(t!(
                        "about.author",
                        author = "Moreno Razzoli a.k.a. Morrolinux"
                    ));
                });
            });
    }

    /// App icon texture, loaded the first time the about box is opened.
    fn about_icon(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if self.about_icon.is_none() {
            let icon = app_icon()?;
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            self.about_icon =
                Some(ctx.load_texture("app_icon", image, egui::TextureOptions::LINEAR));
        }
        self.about_icon.clone()
    }

    fn recent_projects_menu(&mut self, ui: &mut egui::Ui) {
        let recent = self.settings.recent_projects.clone();
        let mut chosen = None;
        ui.add_enabled_ui(!recent.is_empty(), |ui| {
            ui.menu_button(t!("menu.recent_projects"), |ui| {
                menu_list::menu_list(ui, |ui, type_ahead| {
                    for path in &recent {
                        let label = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string());
                        let response = ui.button(&label).on_hover_text(path.display().to_string());
                        type_ahead.item(&response, &label);
                        if response.clicked() {
                            chosen = Some(path.clone());
                        }
                    }
                });
            });
        });
        if let Some(path) = chosen {
            self.request_project_switch(ProjectSwitch::OpenPath(path));
            ui.close();
        }
    }

    pub(crate) fn show_project_settings(&mut self, ctx: &egui::Context) {
        use vv_core::ProcessingPrecision::{High, Standard};
        let current = self.session.project.precision;
        let float_work = self.compositor.float_work_supported();
        let mut chosen = None;
        egui::Window::new(t!("project.settings_title"))
            .open(&mut self.project_settings_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(t!("project.precision")).strong());
                ui.label(egui::RichText::new(t!("project.precision_hint")).weak());
                for (precision, label, hint) in [
                    (
                        High,
                        t!("project.precision_high"),
                        t!("project.precision_high_hint"),
                    ),
                    (
                        Standard,
                        t!("project.precision_standard"),
                        t!("project.precision_standard_hint"),
                    ),
                ] {
                    if ui
                        .radio(precision == current, label)
                        .on_hover_text(hint)
                        .clicked()
                        && precision != current
                    {
                        chosen = Some(precision);
                    }
                }
                if !float_work {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        t!("project.precision_unsupported"),
                    );
                }
            });
        if let Some(precision) = chosen {
            self.session
                .apply(Box::new(vv_core::SetProcessingPrecision::new(precision)));
        }
    }

    /// As in Blender: newest first, the dot on the current state and one
    /// click to jump to any point, forwards or backwards.
    fn undo_history_menu(&mut self, ui: &mut egui::Ui) {
        let labels: Vec<vv_core::CommandLabel> = self.session.history.labels().collect();
        let current = self.session.history.position();
        let mut jump = None;
        ui.add_enabled_ui(!labels.is_empty(), |ui| {
            ui.menu_button(t!("menu.undo_history"), |ui| {
                menu_list::menu_list(ui, |ui, type_ahead| {
                    for position in (0..=labels.len()).rev() {
                        let text = match position {
                            0 => t!("history.original"),
                            _ => command_label(labels[position - 1]),
                        };
                        let response = ui.radio(position == current, text.as_ref());
                        type_ahead.item(&response, &text);
                        if response.clicked() {
                            jump = Some(position);
                        }
                    }
                });
            });
        });
        if let Some(position) = jump {
            self.go_to_history(position);
            ui.close();
        }
    }
}

fn command_label(label: vv_core::CommandLabel) -> std::borrow::Cow<'static, str> {
    use vv_core::CommandLabel as L;
    match label {
        L::AddTrack => t!("history.add_track"),
        L::RemoveTrack => t!("history.remove_track"),
        L::MuteTrack => t!("history.mute_track"),
        L::SoloTrack => t!("history.solo_track"),
        L::LockTrack => t!("history.lock_track"),
        L::ToggleClipsDisabled => t!("history.toggle_clips_disabled"),
        L::InsertClips => t!("history.insert_clips"),
        L::PasteClips => t!("history.paste_clips"),
        L::PasteMedia => t!("history.paste_media"),
        L::DuplicateClips => t!("history.duplicate_clips"),
        L::DeleteClips => t!("history.delete_clips"),
        L::RippleDelete => t!("history.ripple_delete"),
        L::MoveClips => t!("history.move_clips"),
        L::TrimClips => t!("history.trim_clips"),
        L::SlipClips => t!("history.slip_clips"),
        L::UnlinkClips => t!("history.unlink_clips"),
        L::LinkClips => t!("history.link_clips"),
        L::SplitClips => t!("history.split_clips"),
        L::Fade => t!("history.fade"),
        L::Transform => t!("history.transform"),
        L::Flip => t!("history.flip"),
        L::Gain => t!("history.gain"),
        L::ResetGain => t!("history.reset_gain"),
        L::ArmTrack => t!("history.arm_track"),
        L::RecordVoiceover => t!("history.record_voiceover"),
        L::MixerGain => t!("history.mixer_gain"),
        L::MixerPan => t!("history.mixer_pan"),
        L::AddAudioEffect => t!("history.add_audio_effect"),
        L::RemoveAudioEffect => t!("history.remove_audio_effect"),
        L::MoveAudioEffect => t!("history.move_audio_effect"),
        L::EditAudioEffect => t!("history.edit_audio_effect"),
        L::ToggleAudioEffect => t!("history.toggle_audio_effect"),
        L::Title => t!("history.title"),
        L::ResetTransform => t!("history.reset_transform"),
        L::TimelineResolution => t!("history.timeline_resolution"),
        L::TimelineFps => t!("history.timeline_fps"),
        L::SetCaptions => t!("history.set_captions"),
        L::LinkSubtitles => t!("history.link_subtitles"),
        L::SetCover => t!("history.set_cover"),
        L::ClipColor => t!("history.clip_color"),
        L::ClipDisplayColor => t!("history.clip_display_color"),
        L::SetKeyframe => t!("history.set_keyframe"),
        L::RemoveKeyframe => t!("history.remove_keyframe"),
        L::RemoveMedia => t!("history.remove_media"),
        L::RelinkMedia => t!("history.relink_media"),
        L::Filters => t!("history.filters"),
        L::Masks => t!("history.masks"),
        L::BlendMode => t!("history.blend_mode"),
        L::RemoveBackground => t!("history.remove_background"),
        L::Canvas => t!("history.canvas"),
        L::Matte => t!("history.matte"),
        L::Transition => t!("history.transition"),
        L::MakeCompoundClip => t!("history.make_compound_clip"),
        L::UngroupCompoundClip => t!("history.ungroup_compound_clip"),
        L::MoveKeyframes => t!("history.move_keyframes"),
        L::SetInterpolation => t!("history.set_interpolation"),
        L::PasteAttributes => t!("history.paste_attributes"),
        L::ApplyPreset => t!("history.apply_preset"),
        L::ClipSpeed => t!("history.clip_speed"),
        L::SpeedCurve => t!("history.speed_curve"),
        L::Stabilize => t!("history.stabilize"),
        L::FreezeFrame => t!("history.freeze_frame"),
        L::ReverseClips => t!("history.reverse_clips"),
        L::VoiceEffect => t!("props.voice_changer"),
        L::RemoveSilences => t!("history.remove_silences"),
        L::AutoCut => t!("auto_cut.title"),
        L::TextCut => t!("transcript.history_cut"),
        L::CorrectWord => t!("transcript.history_correct"),
        L::ApplyStylePack => t!("style_pack.history_apply"),
        L::AutoZoom => t!("auto_zoom.title"),
        L::RemoveAutoZoom => t!("auto_zoom.remove"),
        L::Reframe => t!("reframe.title"),
        L::RemoveReframe => t!("reframe.remove"),
        L::Dub => t!("dub.history"),
        L::DubLanguage => t!("dub.history_language"),
        L::BeatMarkers => t!("beat.markers_step"),
        L::BeatCuts => t!("beat.cuts_title"),
        L::AddMarker => t!("history.add_marker"),
        L::EditMarker => t!("history.edit_marker"),
        L::MoveMarker => t!("history.move_marker"),
        L::DeleteMarker => t!("history.delete_marker"),
        L::ImportMedia => t!("history.import_media"),
        L::ImportOtio => t!("history.import_otio"),
        L::NewTimeline => t!("history.new_timeline"),
        L::DuplicateTimeline => t!("history.duplicate_timeline"),
        L::RenameTimeline => t!("history.rename_timeline"),
        L::NewFolder => t!("history.new_folder"),
        L::RenameFolder => t!("history.rename_folder"),
        L::MoveToFolder => t!("history.move_to_folder"),
        L::DeleteFolder => t!("history.delete_folder"),
        L::ProcessingPrecision => t!("history.processing_precision"),
    }
}

/// egui's `MenuBar` only opens menus on click; once one is open, hovering
/// another bar entry should switch to it like in native menu bars.
fn switch_bar_menu_on_hover(ctx: &egui::Context, bar_menus: &[egui::Response]) {
    let popup_id = egui::Popup::default_response_id;
    if !bar_menus
        .iter()
        .any(|r| egui::Popup::is_id_open(ctx, popup_id(r)))
    {
        return;
    }
    if let Some(hovered) = bar_menus
        .iter()
        .find(|r| r.hovered() && !egui::Popup::is_id_open(ctx, popup_id(r)))
    {
        egui::Popup::open_id(ctx, popup_id(hovered));
        ctx.request_repaint();
    }
}

#[cfg(test)]
#[path = "tests/app_menu.rs"]
mod tests;
