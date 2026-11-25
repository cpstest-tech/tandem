//! «Stili» tab: the style pack library, the parts to apply with a checkbox
//! each, the preview → Conferma/Annulla step and the pack editor.

use std::collections::HashMap;

use vv_core::style_pack::{
    Animations, Bumper, Component, Music, SafeZone, StylePack, SubtitleStyle, TransitionStyle,
};
use vv_render::assets::AssetKind;

use crate::project_io::DialogKind;
use crate::style_packs::{Outcome, component_label};
use crate::subtitles_ui::template_name;
use crate::{VenturiApp, theme};

const PREVIEW_SIDE: f32 = 88.0;

/// Stills of the subtitle look of each pack, by subtitle template id.
#[derive(Default)]
pub(crate) struct Previews(HashMap<String, Option<egui::TextureHandle>>);

fn asset_name(id: &str) -> String {
    let lang = rust_i18n::locale().to_string();
    vv_render::assets::library()
        .get(id)
        .map_or_else(|| id.to_owned(), |a| a.name.get(&lang).to_owned())
}

fn text_template_name(app: &VenturiApp, id: &str) -> String {
    crate::text_templates::find(id)
        .map(|d| d.name.clone())
        .or_else(|| {
            app.settings
                .text_templates
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.name.clone())
        })
        .unwrap_or_else(|| id.to_owned())
}

impl VenturiApp {
    /// One line telling what a component of `pack` does.
    fn component_summary(&self, pack: &StylePack, component: Component) -> String {
        match component {
            Component::Subtitles => pack
                .subtitles
                .as_ref()
                .map(|s| template_name(&s.template).into_owned())
                .unwrap_or_default(),
            Component::AutoZoom => pack
                .auto_zoom
                .as_ref()
                .map(|z| {
                    t!(
                        "style_pack.zoom_summary",
                        percent = ((z.intensity - 1.0) * 100.0).round() as i32,
                        gap = format!("{:.1}", z.min_gap)
                    )
                    .into_owned()
                })
                .unwrap_or_default(),
            Component::Animations => pack
                .animations
                .as_ref()
                .map(|a| {
                    let parts: Vec<String> = [
                        a.clip_in.as_deref().map(asset_name),
                        a.clip_out.as_deref().map(asset_name),
                        a.text_in.as_ref().map(|id| {
                            t!(
                                "style_pack.text_anim",
                                name = t!(format!("anim.preset.{id}"))
                            )
                            .into_owned()
                        }),
                        a.text_out.as_ref().map(|id| {
                            t!(
                                "style_pack.text_anim",
                                name = t!(format!("anim.preset.{id}"))
                            )
                            .into_owned()
                        }),
                    ]
                    .into_iter()
                    .flatten()
                    .collect();
                    parts.join(" · ")
                })
                .unwrap_or_default(),
            Component::Music => pack
                .music
                .as_ref()
                .map(|m| {
                    let mut text = t!(
                        "style_pack.music_summary",
                        volume = format!("{:.0}", m.volume_db),
                        ducking = format!("{:.0}", m.ducking_db)
                    )
                    .into_owned();
                    if let Some(name) = m.file.as_ref().and_then(|f| f.file_name()) {
                        text = format!("{} · {text}", name.to_string_lossy());
                    }
                    text
                })
                .unwrap_or_default(),
            Component::Transition => pack
                .transition
                .as_ref()
                .map(|t| format!("{} · {:.1} s", asset_name(&t.asset), t.seconds))
                .unwrap_or_default(),
            Component::Intro | Component::Outro => if component == Component::Intro {
                pack.intro.as_ref()
            } else {
                pack.outro.as_ref()
            }
            .map(|b| {
                let name = text_template_name(self, &b.template);
                if b.text.trim().is_empty() {
                    name
                } else {
                    format!("{name} · «{}»", b.text.trim())
                }
            })
            .unwrap_or_default(),
            Component::SafeZone => pack
                .safe_zone
                .as_ref()
                .map(|z| {
                    self.safe_zone_platforms
                        .0
                        .iter()
                        .find(|p| p.id == z.platform)
                        .map_or_else(|| z.platform.clone(), |p| p.name.clone())
                })
                .unwrap_or_default(),
        }
    }

    fn pack_preview_texture(
        &mut self,
        ctx: &egui::Context,
        pack: &StylePack,
    ) -> Option<egui::TextureId> {
        let id = pack.subtitles.as_ref()?.template.clone();
        if !self.style_pack_previews.0.contains_key(&id) {
            let image = self.subtitle_style_image(&id, &t!("style_pack.sample"));
            let texture = image.map(|image| {
                ctx.load_texture(
                    format!("style-pack-{id}"),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            self.style_pack_previews.0.insert(id.clone(), texture);
        }
        self.style_pack_previews
            .0
            .get(&id)?
            .as_ref()
            .map(|t| t.id())
    }

    pub(crate) fn show_style_packs_tab(&mut self, ui: &mut egui::Ui) {
        self.ensure_style_packs();
        self.settle_style_pack_preview();
        if self.style_packs.editor.is_some() {
            self.show_style_pack_editor(ui);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(t!("style_pack.save_current"))
                .on_hover_text(t!("style_pack.save_current_hint"))
                .clicked()
            {
                let pack = self.capture_style_pack();
                let id = pack.id.clone();
                let result = self.save_style_pack(pack);
                if result.is_ok() {
                    self.select_style_pack(&id);
                }
                self.report_pack_result(result, t!("style_pack.saved").into_owned());
            }
            if ui.button(t!("style_pack.import")).clicked() {
                self.spawn_file_dialog(DialogKind::ImportStylePack, |dlg| {
                    dlg.add_filter(t!("style_pack.file_filter"), &["json"])
                        .pick_file()
                });
            }
            if ui
                .small_button("⟳")
                .on_hover_text(t!("style_pack.reload"))
                .clicked()
            {
                self.reload_style_packs();
            }
        });
        if let Some((error, text)) = &self.style_packs.message {
            ui.colored_label(if *error { theme::ERROR } else { theme::SUCCESS }, text);
        }
        if !self.style_packs.load_errors.is_empty() {
            egui::CollapsingHeader::new(
                egui::RichText::new(t!(
                    "style_pack.load_errors",
                    count = self.style_packs.load_errors.len()
                ))
                .color(theme::WARNING),
            )
            .id_salt("style_pack_load_errors")
            .show(ui, |ui| {
                for line in &self.style_packs.load_errors {
                    ui.weak(line);
                }
            });
        }
        if self.timeline_id.is_none() {
            ui.weak(t!("style_pack.no_timeline"));
        }
        ui.add_space(theme::SPACE_1);
        let ids: Vec<String> = self
            .style_packs
            .entries
            .iter()
            .map(|e| e.pack.id.clone())
            .collect();
        egui::ScrollArea::vertical()
            .id_salt("style_packs")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for id in ids {
                    self.style_pack_card(ui, &id);
                    ui.add_space(theme::SPACE_1);
                }
            });
    }

    fn style_pack_card(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some(entry) = self.style_pack(id) else {
            return;
        };
        let pack = entry.pack.clone();
        let builtin = entry.path.is_none();
        let selected = self.style_packs.selected.as_deref() == Some(id);
        let texture = self.pack_preview_texture(ui.ctx(), &pack);
        let frame = egui::Frame::NONE
            .fill(if selected {
                theme::BG_SELECTED
            } else {
                theme::BG_RAISED
            })
            .corner_radius(theme::RADIUS_PANEL)
            .inner_margin(theme::SPACE_2);
        let response = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let text_width = (ui.available_width() - PREVIEW_SIDE - theme::SPACE_2).max(60.0);
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui
                        .allocate_exact_size(egui::Vec2::splat(PREVIEW_SIDE), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, theme::RADIUS_CONTROL, theme::BG_INSET);
                    if let Some(texture) = texture {
                        ui.painter().image(
                            texture,
                            rect,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                    }
                    ui.vertical(|ui| {
                        ui.set_max_width(text_width);
                        ui.horizontal(|ui| {
                            ui.strong(&pack.name);
                            if builtin {
                                ui.weak(t!("style_pack.builtin"));
                            }
                        });
                        if !pack.description.is_empty() {
                            ui.add(
                                egui::Label::new(egui::RichText::new(&pack.description).weak())
                                    .wrap(),
                            );
                        }
                        let parts: Vec<String> = pack
                            .components()
                            .into_iter()
                            .map(|c| component_label(c).into_owned())
                            .collect();
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(parts.join(" · "))
                                    .small()
                                    .color(theme::ACCENT),
                            )
                            .wrap(),
                        );
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        if response.clicked() && !selected {
            self.select_style_pack(id);
        }
        if selected {
            self.style_pack_details(ui, &pack, builtin);
        }
    }

    fn style_pack_details(&mut self, ui: &mut egui::Ui, pack: &StylePack, builtin: bool) {
        let missing = self.missing_pack_parts(pack);
        let previewing = self
            .style_packs
            .preview
            .as_ref()
            .is_some_and(|p| p.pack == pack.id);
        ui.add_space(theme::SPACE_1);
        ui.label(egui::RichText::new(t!("style_pack.choose_parts")).strong());
        let mut changed = false;
        for component in pack.components() {
            let why = missing
                .iter()
                .find(|(c, _)| *c == component)
                .map(|(_, w)| w);
            ui.horizontal_wrapped(|ui| {
                let mut on = self.style_packs.choice.contains(&component);
                if ui
                    .add_enabled(
                        why.is_none(),
                        egui::Checkbox::new(&mut on, component_label(component)),
                    )
                    .changed()
                {
                    if on {
                        self.style_packs.choice.insert(component);
                    } else {
                        self.style_packs.choice.remove(&component);
                    }
                    changed = true;
                }
                ui.weak(self.component_summary(pack, component));
            });
            if let Some(why) = why {
                ui.colored_label(theme::WARNING, why);
            }
        }
        if self.style_packs.choice.contains(&Component::SafeZone) {
            ui.weak(t!("style_pack.safe_zone_note"));
        }
        ui.add_space(theme::SPACE_1);
        if previewing {
            // Ticking a part re-applies once the pointer is up, as auto zoom does.
            self.style_packs.dirty |= changed;
            if self.style_packs.dirty && !ui.input(|i| i.pointer.any_down()) {
                self.style_packs.dirty = false;
                self.preview_style_pack();
            }
            if let Some(preview) = &self.style_packs.preview {
                for (component, outcome) in &preview.report {
                    let (color, text) = match outcome {
                        Outcome::Applied(text) => (theme::SUCCESS, text),
                        Outcome::Skipped(text) => (theme::WARNING, text),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(color, component_label(*component));
                        ui.label(text);
                    });
                }
            }
            ui.weak(t!("style_pack.preview_hint"));
            ui.horizontal(|ui| {
                let confirm = crate::ui_kit::primary_button(t!("style_pack.confirm"));
                if ui.add(confirm).clicked() {
                    self.confirm_style_pack();
                }
                if ui.button(t!("common.cancel")).clicked() {
                    self.cancel_style_pack();
                }
            });
        } else {
            let can_apply = self.timeline_id.is_some() && !self.style_packs.choice.is_empty();
            let apply = crate::ui_kit::primary_button(t!("style_pack.preview"));
            if ui
                .add_enabled(can_apply, apply)
                .on_hover_text(t!("style_pack.preview_button_hint"))
                .clicked()
            {
                self.preview_style_pack();
            }
        }
        ui.add_space(theme::SPACE_1);
        let id = pack.id.clone();
        if let Some((rename_id, text)) = &mut self.style_packs.rename
            && *rename_id == id
        {
            let mut done = None;
            ui.horizontal(|ui| {
                let edit = ui.text_edit_singleline(text);
                edit.request_focus();
                if ui.button(t!("style_pack.ok")).clicked()
                    || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    done = Some(true);
                }
                if ui.button(t!("common.cancel")).clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    done = Some(false);
                }
            });
            match done {
                Some(true) => {
                    let name = text.clone();
                    self.style_packs.rename = None;
                    self.rename_style_pack(&id, &name);
                }
                Some(false) => self.style_packs.rename = None,
                None => {}
            }
            return;
        }
        if self.style_packs.confirm_delete.as_deref() == Some(id.as_str()) {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(
                    theme::ERROR,
                    t!("style_pack.delete_confirm", name = &pack.name),
                );
                if ui.button(t!("style_pack.delete")).clicked() {
                    self.style_packs.confirm_delete = None;
                    self.delete_style_pack(&id);
                }
                if ui.button(t!("common.cancel")).clicked() {
                    self.style_packs.confirm_delete = None;
                }
            });
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(t!("style_pack.duplicate")).clicked() {
                self.duplicate_style_pack(&id);
            }
            if ui
                .add_enabled(!builtin, egui::Button::new(t!("style_pack.rename")))
                .on_disabled_hover_text(t!("style_pack.builtin_locked"))
                .clicked()
            {
                self.style_packs.rename = Some((id.clone(), pack.name.clone()));
            }
            if ui
                .add_enabled(!builtin, egui::Button::new(t!("style_pack.edit")))
                .on_disabled_hover_text(t!("style_pack.builtin_locked"))
                .clicked()
            {
                self.style_packs.editor = Some(pack.clone());
            }
            if ui.button(t!("style_pack.export")).clicked() {
                let name = format!("{}.json", crate::export_plan::sanitize_name(&pack.name));
                let kind = DialogKind::ExportStylePack(id.clone());
                let filter = t!("style_pack.file_filter").into_owned();
                self.spawn_file_dialog(kind, move |dlg| {
                    dlg.add_filter(filter, &["json"])
                        .set_file_name(name)
                        .save_file()
                });
            }
            if ui
                .add_enabled(!builtin, egui::Button::new(t!("style_pack.delete")))
                .on_disabled_hover_text(t!("style_pack.builtin_locked"))
                .clicked()
            {
                self.style_packs.confirm_delete = Some(id.clone());
            }
        });
    }

    fn show_style_pack_editor(&mut self, ui: &mut egui::Ui) {
        let Some(mut pack) = self.style_packs.editor.take() else {
            return;
        };
        let mut close = None;
        ui.horizontal(|ui| {
            if ui.button("←").on_hover_text(t!("common.cancel")).clicked() {
                close = Some(false);
            }
            ui.strong(t!("style_pack.editor_title"));
        });
        egui::ScrollArea::vertical()
            .id_salt("style_pack_editor")
            .auto_shrink([false, false])
            .max_height(ui.available_height() - theme::TOUCH_CONTROL_HEIGHT - theme::SPACE_3)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(t!("style_pack.name"));
                    ui.text_edit_singleline(&mut pack.name);
                });
                ui.label(t!("style_pack.description"));
                ui.text_edit_multiline(&mut pack.description);
                ui.separator();
                self.edit_subtitles(ui, &mut pack.subtitles);
                edit_auto_zoom(ui, &mut pack.auto_zoom);
                edit_animations(ui, &mut pack.animations);
                self.edit_music(ui, &mut pack.music);
                edit_transition(ui, &mut pack.transition);
                self.edit_bumper(ui, Component::Intro, &mut pack.intro);
                self.edit_bumper(ui, Component::Outro, &mut pack.outro);
                self.edit_safe_zone(ui, &mut pack.safe_zone);
            });
        ui.horizontal(|ui| {
            let save = crate::ui_kit::primary_button(t!("style_pack.save"));
            if ui.add_enabled(!pack.name.trim().is_empty(), save).clicked() {
                close = Some(true);
            }
            if ui.button(t!("common.cancel")).clicked() {
                close = Some(false);
            }
        });
        match close {
            Some(true) => {
                let id = pack.id.clone();
                pack.name = pack.name.trim().to_owned();
                let result = self.save_style_pack(pack);
                if result.is_ok() {
                    self.select_style_pack(&id);
                }
                self.report_pack_result(result, t!("style_pack.saved").into_owned());
            }
            Some(false) => {}
            None => self.style_packs.editor = Some(pack),
        }
    }

    fn edit_subtitles(&self, ui: &mut egui::Ui, value: &mut Option<SubtitleStyle>) {
        if !include(ui, Component::Subtitles, value, || SubtitleStyle {
            template: vv_core::captions::DEFAULT_TEMPLATE.into(),
        }) {
            return;
        }
        let style = value.as_mut().unwrap();
        let prefix = crate::text_templates::SUBTITLE_PREFIX;
        let mut choices: Vec<(String, String)> = vv_core::captions::TEMPLATES
            .iter()
            .map(|t| (t.id.to_owned(), template_name(t.id).into_owned()))
            .collect();
        choices.extend(
            crate::text_templates::all()
                .iter()
                .map(|d| (format!("{prefix}{}", d.id), d.name.clone())),
        );
        choices.extend(
            self.settings
                .text_templates
                .iter()
                .map(|t| (format!("{prefix}{}", t.id), t.name.clone())),
        );
        combo(ui, "pack_subtitles", &mut style.template, &choices);
    }

    fn edit_music(&mut self, ui: &mut egui::Ui, value: &mut Option<Music>) {
        if !include(ui, Component::Music, value, Music::default) {
            return;
        }
        let music = value.as_mut().unwrap();
        ui.horizontal_wrapped(|ui| {
            match &music.file {
                Some(file) => ui.label(file.file_name().map_or_else(
                    || file.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )),
                None => ui.weak(t!("style_pack.music_existing")),
            };
            if ui.button(t!("style_pack.music_pick")).clicked() {
                self.spawn_file_dialog(DialogKind::StylePackMusic, |dlg| dlg.pick_file());
            }
            if music.file.is_some() && ui.button(t!("style_pack.music_clear")).clicked() {
                music.file = None;
            }
        });
        ui.add(
            egui::Slider::new(&mut music.volume_db, -30.0..=0.0)
                .text(t!("style_pack.music_volume"))
                .suffix(" dB"),
        );
        ui.add(
            egui::Slider::new(&mut music.ducking_db, -30.0..=0.0)
                .text(t!("style_pack.music_ducking"))
                .suffix(" dB"),
        );
    }

    fn edit_bumper(&self, ui: &mut egui::Ui, which: Component, value: &mut Option<Bumper>) {
        let first = crate::text_templates::all()
            .first()
            .map(|d| d.id.clone())
            .unwrap_or_default();
        if !include(ui, which, value, || Bumper {
            template: first,
            text: String::new(),
        }) {
            return;
        }
        let bumper = value.as_mut().unwrap();
        let mut choices: Vec<(String, String)> = crate::text_templates::all()
            .iter()
            .map(|d| (d.id.clone(), d.name.clone()))
            .collect();
        choices.extend(
            self.settings
                .text_templates
                .iter()
                .map(|t| (t.id.clone(), t.name.clone())),
        );
        combo(ui, (which, "bumper"), &mut bumper.template, &choices);
        ui.horizontal(|ui| {
            ui.label(t!("style_pack.bumper_text"));
            ui.add(
                egui::TextEdit::singleline(&mut bumper.text)
                    .hint_text(t!("style_pack.bumper_text_hint")),
            );
        });
    }

    fn edit_safe_zone(&self, ui: &mut egui::Ui, value: &mut Option<SafeZone>) {
        let first = self
            .safe_zone_platforms
            .0
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default();
        if !include(ui, Component::SafeZone, value, || SafeZone {
            platform: first,
        }) {
            return;
        }
        let zone = value.as_mut().unwrap();
        let choices: Vec<(String, String)> = self
            .safe_zone_platforms
            .0
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        combo(ui, "pack_safe_zone", &mut zone.platform, &choices);
    }
}

/// The «include» checkbox of a component; `true` while it is included.
fn include<T>(
    ui: &mut egui::Ui,
    component: Component,
    value: &mut Option<T>,
    default: impl FnOnce() -> T,
) -> bool {
    ui.add_space(theme::SPACE_1);
    let mut on = value.is_some();
    if ui
        .checkbox(
            &mut on,
            egui::RichText::new(component_label(component)).strong(),
        )
        .changed()
    {
        *value = on.then(default);
    }
    value.is_some()
}

fn combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    choices: &[(String, String)],
) {
    let shown = choices
        .iter()
        .find(|(id, _)| id == value)
        .map_or_else(|| value.clone(), |(_, name)| name.clone());
    egui::ComboBox::from_id_salt(salt)
        .selected_text(shown)
        .width(ui.available_width().min(260.0))
        .show_ui(ui, |ui| {
            for (id, name) in choices {
                ui.selectable_value(value, id.clone(), name);
            }
        });
}

/// Like `combo`, with a «none» entry.
fn optional_combo(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    value: &mut Option<String>,
    choices: &[(String, String)],
) {
    ui.horizontal(|ui| {
        ui.label(label);
        let shown = value
            .as_ref()
            .map(|v| {
                choices
                    .iter()
                    .find(|(id, _)| id == v)
                    .map_or_else(|| v.clone(), |(_, name)| name.clone())
            })
            .unwrap_or_else(|| t!("style_pack.none").into_owned());
        egui::ComboBox::from_id_salt(salt)
            .selected_text(shown)
            .width(ui.available_width().min(220.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(value, None, t!("style_pack.none"));
                for (id, name) in choices {
                    ui.selectable_value(value, Some(id.clone()), name);
                }
            });
    });
}

fn edit_auto_zoom(ui: &mut egui::Ui, value: &mut Option<vv_core::auto_zoom::Config>) {
    use vv_core::auto_zoom::{Frequency, INTENSITY, MIN_GAP, Speed};
    if !include(ui, Component::AutoZoom, value, Default::default) {
        return;
    }
    let config = value.as_mut().unwrap();
    let mut percent = config.intensity * 100.0;
    if ui
        .add(
            egui::Slider::new(
                &mut percent,
                INTENSITY.start() * 100.0..=INTENSITY.end() * 100.0,
            )
            .text(t!("auto_zoom.intensity"))
            .suffix(" %"),
        )
        .changed()
    {
        config.intensity = percent / 100.0;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(t!("auto_zoom.speed"));
        for (speed, label) in [
            (Speed::Slow, t!("auto_zoom.speed_slow")),
            (Speed::Normal, t!("auto_zoom.speed_normal")),
            (Speed::Fast, t!("auto_zoom.speed_fast")),
        ] {
            ui.selectable_value(&mut config.speed, speed, label);
        }
    });
    ui.horizontal_wrapped(|ui| {
        ui.label(t!("auto_zoom.frequency"));
        for (frequency, label) in [
            (Frequency::Low, t!("auto_zoom.frequency_low")),
            (Frequency::Medium, t!("auto_zoom.frequency_medium")),
            (Frequency::High, t!("auto_zoom.frequency_high")),
        ] {
            if ui
                .selectable_label(config.frequency == frequency, label)
                .clicked()
            {
                config.frequency = frequency;
                config.min_gap = frequency.default_gap();
            }
        }
    });
    ui.add(
        egui::Slider::new(&mut config.min_gap, *MIN_GAP.start()..=*MIN_GAP.end())
            .text(t!("auto_zoom.min_gap"))
            .suffix(" s"),
    );
    ui.checkbox(&mut config.alternate, t!("auto_zoom.alternate"));
    ui.checkbox(&mut config.shift, t!("auto_zoom.shift"));
}

fn edit_animations(ui: &mut egui::Ui, value: &mut Option<Animations>) {
    if !include(ui, Component::Animations, value, Animations::default) {
        return;
    }
    let anim = value.as_mut().unwrap();
    let lang = rust_i18n::locale().to_string();
    let library = vv_render::assets::library();
    let assets = |category: &str| -> Vec<(String, String)> {
        library
            .of_kind(AssetKind::Animation)
            .filter(|a| a.category == category)
            .map(|a| (a.id.clone(), a.name.get(&lang).to_owned()))
            .collect()
    };
    let presets = |ids: Vec<&'static str>| -> Vec<(String, String)> {
        ids.into_iter()
            .map(|id| (id.to_owned(), t!(format!("anim.preset.{id}")).into_owned()))
            .collect()
    };
    optional_combo(
        ui,
        "pack_clip_in",
        &t!("style_pack.clip_in"),
        &mut anim.clip_in,
        &assets("in"),
    );
    if anim.clip_in.is_some() {
        ui.add(
            egui::Slider::new(&mut anim.clip_in_secs, 0.05..=3.0)
                .text(t!("props.anim_duration"))
                .suffix(" s"),
        );
    }
    optional_combo(
        ui,
        "pack_clip_out",
        &t!("style_pack.clip_out"),
        &mut anim.clip_out,
        &assets("out"),
    );
    if anim.clip_out.is_some() {
        ui.add(
            egui::Slider::new(&mut anim.clip_out_secs, 0.05..=3.0)
                .text(t!("props.anim_duration"))
                .suffix(" s"),
        );
    }
    optional_combo(
        ui,
        "pack_text_in",
        &t!("style_pack.text_in"),
        &mut anim.text_in,
        &presets(vv_core::text_anim_presets::IN_IDS.to_vec()),
    );
    optional_combo(
        ui,
        "pack_text_out",
        &t!("style_pack.text_out"),
        &mut anim.text_out,
        &presets(vv_core::text_anim_presets::out_ids()),
    );
}

fn edit_transition(ui: &mut egui::Ui, value: &mut Option<TransitionStyle>) {
    if !include(ui, Component::Transition, value, || TransitionStyle {
        asset: "transitions/dissolve".into(),
        seconds: 0.5,
    }) {
        return;
    }
    let transition = value.as_mut().unwrap();
    let lang = rust_i18n::locale().to_string();
    let choices: Vec<(String, String)> = vv_render::assets::library()
        .of_kind(AssetKind::Transition)
        .map(|a| (a.id.clone(), a.name.get(&lang).to_owned()))
        .collect();
    combo(ui, "pack_transition", &mut transition.asset, &choices);
    ui.add(
        egui::Slider::new(&mut transition.seconds, 0.1..=2.0)
            .text(t!("props.anim_duration"))
            .suffix(" s"),
    );
}
