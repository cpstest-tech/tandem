use super::{JobKind, Message, OriginalVoice, translate::Provider, tts};
use crate::{VenturiApp, theme};
use vv_core::Timeline;
use vv_core::dub::{self, Language};

pub(crate) fn language_label(code: &str) -> std::borrow::Cow<'_, str> {
    match code {
        "it" => t!("dub.lang_it"),
        "en" => t!("dub.lang_en"),
        "es" => t!("dub.lang_es"),
        "fr" => t!("dub.lang_fr"),
        "de" => t!("dub.lang_de"),
        "pt" => t!("dub.lang_pt"),
        _ => code.into(),
    }
}

pub(crate) fn language_combo(
    ui: &mut egui::Ui,
    id: &str,
    timeline: &Timeline,
    choice: &mut Language,
    allow_as_is: bool,
) {
    let languages = dub::languages(timeline);
    if languages.is_empty() {
        return;
    }
    let label = match choice {
        Language::AsIs => t!("dub.as_is"),
        Language::Original => t!("dub.original"),
        Language::Dub(code) => language_label(code),
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .show_ui(ui, |ui| {
            if allow_as_is {
                ui.selectable_value(choice, Language::AsIs, t!("dub.as_is"));
            }
            ui.selectable_value(choice, Language::Original, t!("dub.original"));
            for code in languages {
                let label = language_label(&code).into_owned();
                ui.selectable_value(choice, Language::Dub(code), label);
            }
        });
}

enum Action {
    Translate,
    Synthesize(Option<Vec<usize>>),
    Shorten(usize),
    RetryTranslation(usize),
    Preview,
    Play(usize),
    Confirm,
    Discard,
    Install,
    InstallWhisper,
    CancelDownload,
    Close,
}

impl VenturiApp {
    pub(crate) fn show_dub(&mut self, ctx: &egui::Context) {
        if !self.dub.open {
            return;
        }
        let Some(timeline) = self.timeline_id else {
            self.dub.open = false;
            return;
        };
        let pending = self.dub_preview_pending();
        let working = self.dub.busy().is_some()
            || self.dub.installing().is_some()
            || self.dub.whisper.is_some();
        if !pending && !working {
            self.refresh_dub_rows();
            self.poll_transcript_jobs(ctx);
        }
        let mut action = None;
        let before = self.settings.shorts.dub.clone();
        let modal = egui::Modal::new(egui::Id::new("dub_modal")).show(ctx, |ui| {
            let size = ctx.content_rect().size();
            ui.set_width((size.x - theme::SPACE_5 * 2.0).clamp(220.0, 820.0));
            crate::ui_kit::heading(ui, t!("dub.title"));
            if self.dub.confirm_whisper {
                if let Some((_, model)) = self.dub_whisper_missing() {
                    ui.label(t!("dub.whisper_download_hint", mb = model.size_mb()));
                }
                ui.horizontal(|ui| {
                    if ui.button(t!("dub.download")).clicked() {
                        action = Some(Action::InstallWhisper);
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        action = Some(Action::CancelDownload);
                    }
                });
                return;
            }
            if self.dub.confirm_install {
                self.dub_download_prompt(ui, &mut action);
                return;
            }
            egui::ScrollArea::vertical()
                .max_height((size.y - theme::CONTROL_HEIGHT * 5.0).max(theme::FIELD_HEIGHT))
                .show(ui, |ui| {
                    if pending {
                        ui.label(t!("dub.preview_ready"));
                        ui.horizontal_wrapped(|ui| {
                            let key = if self.is_timeline_playing() {
                                t!("dub.pause")
                            } else {
                                t!("dub.play")
                            };
                            if ui.button(key).clicked() {
                                self.toggle_playback();
                            }
                            egui::ComboBox::from_id_salt("dub_preview_phrase")
                                .selected_text(t!("dub.seek_phrase"))
                                .show_ui(ui, |ui| {
                                    for (index, row) in self.dub.rows.iter().enumerate() {
                                        if ui
                                            .selectable_label(
                                                false,
                                                format!("{:.2} s  {}", row.start, row.text),
                                            )
                                            .clicked()
                                        {
                                            action = Some(Action::Play(index));
                                        }
                                    }
                                });
                            if ui.button(t!("dub.apply")).clicked() {
                                action = Some(Action::Confirm);
                            }
                            if ui.button(t!("common.cancel")).clicked() {
                                action = Some(Action::Discard);
                            }
                        });
                    } else {
                        ui.add_enabled_ui(!working, |ui| {
                            let mut choice = self.playing_dub(timeline);
                            let previous = choice.clone();
                            language_combo(
                                ui,
                                "editor_dub_language",
                                &self.session.project.timelines[timeline],
                                &mut choice,
                                false,
                            );
                            if choice != previous {
                                self.apply_dub_language(timeline, &choice);
                            }
                            self.dub_options(ui);
                        });
                        if !working {
                            if self.dub_whisper_missing().is_some()
                                && matches!(
                                    self.transcript_job_status(),
                                    crate::transcript::JobStatus::Idle
                                )
                            {
                                if ui.button(t!("dub.get_whisper")).clicked() {
                                    self.dub.confirm_whisper = true;
                                }
                            } else {
                                self.transcript_status(ui);
                            }
                        }
                        if self.settings.shorts.dub.language == before.language {
                            self.dub_review(ui, working, &mut action);
                        }
                    }
                    self.dub_progress(ui);
                    if let Some(message) = &self.dub.message {
                        match message {
                            Message::Info(text) => {
                                ui.label(text);
                            }
                            Message::Error(text) => {
                                ui.colored_label(theme::ERROR, text);
                            }
                        }
                    }
                });
            if !pending && !working && ui.button(t!("dub.close")).clicked() {
                action = Some(Action::Close);
            }
        });
        if modal.should_close() && action.is_none() && !working {
            action = Some(if self.dub.confirm_install || self.dub.confirm_whisper {
                Action::CancelDownload
            } else if pending {
                Action::Discard
            } else {
                Action::Close
            });
        }
        if self.settings.shorts.dub != before {
            if self.settings.shorts.dub.language != before.language {
                self.refresh_dub_rows();
            } else if self.settings.shorts.dub.translator != before.translator {
                self.fill_cached_translations();
            }
            self.persist_settings();
        }
        match action {
            Some(Action::Translate) => self.start_dub_translation(None, false),
            Some(Action::Shorten(row)) => self.start_dub_translation(Some(vec![row]), true),
            Some(Action::RetryTranslation(row)) => {
                self.start_dub_translation(Some(vec![row]), false)
            }
            Some(Action::Synthesize(rows)) => self.start_dub_speech(rows),
            Some(Action::Preview) => self.start_dub_preview(),
            Some(Action::Play(row)) => self.listen_dub_row(row),
            Some(Action::Confirm) => {
                self.confirm_dub();
                self.dub.open = false;
            }
            Some(Action::Discard) => {
                if self.is_timeline_playing() {
                    self.toggle_playback();
                }
                self.discard_dub();
            }
            Some(Action::Install) => self.start_dub_install(),
            Some(Action::InstallWhisper) => self.start_dub_whisper_install(),
            Some(Action::CancelDownload) => {
                self.dub.confirm_install = false;
                self.dub.confirm_whisper = false;
            }
            Some(Action::Close) => self.dub.open = false,
            None => {}
        }
    }

    fn dub_options(&mut self, ui: &mut egui::Ui) {
        let prefs = &mut self.settings.shorts.dub;
        if !tts::languages().contains(&prefs.language.as_str()) {
            prefs.language = "en".into();
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(t!("dub.target_language"));
            egui::ComboBox::from_id_salt("dub_target_language")
                .selected_text(language_label(&prefs.language))
                .show_ui(ui, |ui| {
                    for code in tts::languages() {
                        ui.selectable_value(
                            &mut prefs.language,
                            code.to_owned(),
                            language_label(code),
                        );
                    }
                });
            let mut voices = tts::voices_for(&prefs.language).peekable();
            if !tts::find_voice(&prefs.voice).is_some_and(|v| v.language == prefs.language)
                && let Some(voice) = voices.peek()
            {
                prefs.voice = voice.id.to_owned();
            }
            ui.label(t!("dub.voice"));
            egui::ComboBox::from_id_salt("dub_voice")
                .selected_text(tts::find_voice(&prefs.voice).map_or("", |v| v.name))
                .show_ui(ui, |ui| {
                    for voice in voices {
                        ui.selectable_value(&mut prefs.voice, voice.id.to_owned(), voice.name);
                    }
                });
            ui.label(t!("dub.translator"));
            egui::ComboBox::from_id_salt("dub_provider")
                .selected_text(match prefs.translator {
                    Provider::Manual => t!("dub.manual"),
                    Provider::Gemini => "Gemini".into(),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut prefs.translator, Provider::Manual, t!("dub.manual"));
                    ui.selectable_value(&mut prefs.translator, Provider::Gemini, "Gemini");
                });
        });
        if let Some(voice) = tts::find_voice(&prefs.voice) {
            ui.hyperlink_to(t!("dub.voice_card"), voice.card_url());
        }
        if prefs.translator == Provider::Gemini {
            ui.horizontal(|ui| {
                ui.label(t!("dub.glossary"));
                ui.text_edit_singleline(&mut prefs.glossary);
            });
        }
        egui::CollapsingHeader::new(t!("dub.mix")).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut prefs.original,
                    OriginalVoice::Duck,
                    t!("dub.duck_voice"),
                );
                ui.selectable_value(
                    &mut prefs.original,
                    OriginalVoice::Mute,
                    t!("dub.mute_voice"),
                );
            });
            if prefs.original == OriginalVoice::Duck {
                ui.add(
                    egui::Slider::new(&mut prefs.voice_duck_db, -36.0..=0.0)
                        .suffix(" dB")
                        .text(t!("dub.voice")),
                );
            }
            ui.add(
                egui::Slider::new(&mut prefs.music_duck_db, -36.0..=0.0)
                    .suffix(" dB")
                    .text(t!("dub.music")),
            );
            ui.add(
                egui::Slider::new(&mut prefs.max_stretch, super::MAX_STRETCH)
                    .custom_formatter(|value, _| format!("{:.0}%", value * 100.0))
                    .custom_parser(|value| {
                        value
                            .trim_end_matches('%')
                            .parse::<f64>()
                            .ok()
                            .map(|v| v / 100.0)
                    })
                    .text(t!("dub.max_stretch")),
            );
            ui.checkbox(&mut prefs.subtitles, t!("dub.subtitles"));
            ui.checkbox(&mut prefs.normalize, t!("dub.normalize"));
        });
    }

    fn dub_review(&mut self, ui: &mut egui::Ui, working: bool, action: &mut Option<Action>) {
        if self.dub.rows.is_empty() {
            return;
        }
        let blocked = self.translator_blocked();
        let prefs = &self.settings.shorts.dub;
        let cloud = prefs.translator == Provider::Gemini;
        if cloud && let Some(reason) = &blocked {
            ui.colored_label(theme::WARNING, reason.as_ref());
        }
        if !tts::piper_supported() {
            ui.colored_label(theme::WARNING, t!("dub.piper_unsupported"));
        }
        ui.label(t!("dub.review_hint"));
        let width = ((ui.available_width() - theme::SPACE_5) / 2.0).max(theme::SPACE_5);
        for (index, row) in self.dub.rows.iter_mut().enumerate() {
            let fresh = row.audio_fresh(&prefs.voice);
            let fit = row.fit(prefs.max_stretch).filter(|_| fresh);
            let overflow = fit.is_some_and(|fit| !fit.fits);
            ui.push_id(index, |ui| {
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!("{:.2}–{:.2} s", row.start, row.end));
                    if row.busy {
                        ui.spinner();
                    }
                    if overflow {
                        ui.colored_label(theme::ERROR, t!("dub.too_long"));
                    }
                    if let Some((_, error)) = &row.error {
                        ui.colored_label(theme::ERROR, error);
                    }
                });
                let edit_id = ui.id().with("translation");
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.weak(t!("dub.original"));
                            ui.label(&row.source);
                        },
                    );
                    ui.vertical(|ui| {
                        ui.weak(t!("dub.translation"));
                        if ui
                            .add_enabled(
                                !working,
                                egui::TextEdit::multiline(&mut row.text)
                                    .desired_width(width)
                                    .desired_rows(2),
                            )
                            .changed()
                        {
                            row.translated = !row.text.trim().is_empty();
                            row.audio = None;
                            row.error = None;
                        }
                    });
                });
                ui.add_enabled_ui(!working, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if overflow && ui.small_button(t!("dub.shorten")).clicked() {
                            if cloud && blocked.is_none() {
                                *action = Some(Action::Shorten(index));
                            } else {
                                ui.memory_mut(|m| m.request_focus(edit_id));
                            }
                        }
                        if let Some((kind, _)) = &row.error {
                            let available = *kind == JobKind::Speak || (cloud && blocked.is_none());
                            if available && ui.small_button(t!("dub.retry")).clicked() {
                                *action = match kind {
                                    JobKind::Translate => Some(Action::RetryTranslation(index)),
                                    JobKind::Shorten => Some(Action::Shorten(index)),
                                    JobKind::Speak => Some(Action::Synthesize(Some(vec![index]))),
                                    JobKind::Prepare => None,
                                };
                            }
                        }
                    });
                });
            });
        }
        ui.separator();
        let reviewed = |row: &super::Row| (!cloud || row.translated) && !row.text.trim().is_empty();
        let ready = self.dub.rows.iter().all(reviewed);
        let current_audio = |row: &super::Row| row.audio_fresh(&prefs.voice);
        ui.horizontal_wrapped(|ui| {
            if cloud
                && ui
                    .add_enabled(
                        !working && blocked.is_none(),
                        egui::Button::new(t!("dub.translate")),
                    )
                    .clicked()
            {
                *action = Some(Action::Translate);
            }
            if ui
                .add_enabled(
                    !working
                        && tts::piper_supported()
                        && ready
                        && self.dub.rows.iter().any(|row| !current_audio(row)),
                    egui::Button::new(t!("dub.generate_audio")),
                )
                .clicked()
            {
                *action = Some(Action::Synthesize(None));
            }
            if ui
                .add_enabled(
                    !working
                        && ready
                        && self.dub.rows.iter().all(|row| {
                            current_audio(row)
                                && row.fit(prefs.max_stretch).is_some_and(|fit| fit.fits)
                        }),
                    egui::Button::new(t!("dub.preview")),
                )
                .clicked()
            {
                *action = Some(Action::Preview);
            }
        });
    }

    fn dub_download_prompt(&self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        if let Some((engine, model)) = self.dub_voice_missing() {
            ui.label(t!("dub.download_hint"));
            if let Some(mb) = engine {
                ui.label(t!("dub.download_engine", mb = mb));
            }
            if let Some(mb) = model
                && let Some(voice) = tts::find_voice(&self.settings.shorts.dub.voice)
            {
                ui.label(t!(
                    "dub.download_voice",
                    voice = voice.name,
                    mb = mb,
                    license = voice.license
                ));
            }
        }
        ui.horizontal(|ui| {
            if ui.button(t!("dub.download")).clicked() {
                *action = Some(Action::Install);
            }
            if ui.button(t!("common.cancel")).clicked() {
                *action = Some(Action::CancelDownload);
            }
        });
    }

    fn dub_progress(&self, ui: &mut egui::Ui) {
        let progress = if let Some(whisper) = &self.dub.whisper {
            Some((whisper.progress.fraction(), t!("subtitles.stage_download")))
        } else if let Some(fraction) = self.dub.installing() {
            Some((fraction, t!("dub.downloading")))
        } else if let Some((kind, done, total)) = self.dub.busy() {
            let text = match kind {
                JobKind::Translate | JobKind::Shorten => t!("dub.translating"),
                JobKind::Speak => t!("dub.synthesizing"),
                JobKind::Prepare => t!("dub.preparing"),
            };
            Some((done as f32 / total.max(1) as f32, text))
        } else {
            None
        };
        if let Some((fraction, text)) = progress {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .text(text)
                    .show_percentage(),
            );
            if ui.button(t!("common.cancel")).clicked() {
                self.dub.cancel();
            }
        }
    }
}
