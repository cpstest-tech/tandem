//! Ctrl+K: every action and timeline command, found by typing part of its name.

use crate::settings::Action;
use crate::timeline_ui::TimelineCommand;
use crate::{VenturiApp, theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Entry {
    Action(Action),
    Command(TimelineCommand),
    Broll,
}

/// Timeline commands that have no shortcut of their own.
const COMMANDS: [TimelineCommand; 11] = [
    TimelineCommand::AutoCut,
    TimelineCommand::AutoZoom,
    TimelineCommand::Reframe,
    TimelineCommand::Dub,
    TimelineCommand::Voiceover,
    TimelineCommand::BeatSync,
    TimelineCommand::FreezeFrame,
    TimelineCommand::Reverse,
    TimelineCommand::Mirror,
    TimelineCommand::SyncByAudio,
    TimelineCommand::MatchColor,
];

fn command_label(command: TimelineCommand) -> std::borrow::Cow<'static, str> {
    match command {
        TimelineCommand::AutoCut => t!("auto_cut.title"),
        TimelineCommand::AutoZoom => t!("auto_zoom.title"),
        TimelineCommand::Reframe => t!("reframe.title"),
        TimelineCommand::Dub => t!("dub.title"),
        TimelineCommand::Voiceover => t!("voiceover.title"),
        TimelineCommand::BeatSync => t!("beat.title"),
        TimelineCommand::FreezeFrame => t!("timeline.freeze_frame_action"),
        TimelineCommand::Reverse => t!("timeline.reverse"),
        TimelineCommand::Mirror => t!("timeline.mirror"),
        TimelineCommand::SyncByAudio => t!("sync.menu"),
        TimelineCommand::MatchColor => t!("color_match.menu"),
        _ => "".into(),
    }
}

/// Whether every word of `query` appears in `label`, in any order.
pub(crate) fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| label.contains(word))
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) open: bool,
    query: String,
    selected: usize,
    /// Run on the next frame, once the palette's text field let go of the keyboard.
    pub(crate) pending: Option<Entry>,
}

impl VenturiApp {
    fn palette_entries(&self) -> Vec<(Entry, String)> {
        let keymap = &self.settings.keymap;
        Action::ALL
            .into_iter()
            .filter(|a| *a != Action::CommandPalette)
            .map(|a| (Entry::Action(a), keymap.menu_label(&a.label(), a)))
            .chain(
                COMMANDS
                    .into_iter()
                    .map(|c| (Entry::Command(c), command_label(c).into_owned())),
            )
            .chain([(Entry::Broll, t!("broll.button").into_owned())])
            .collect()
    }

    pub(crate) fn show_command_palette(&mut self, ctx: &egui::Context) {
        if !self.palette.open {
            return;
        }
        let entries: Vec<(Entry, String)> = self
            .palette_entries()
            .into_iter()
            .filter(|(_, label)| matches(label, &self.palette.query))
            .collect();
        let mut chosen = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("command_palette")).show(ctx, |ui| {
            ui.set_width(420.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.palette.query)
                    .hint_text(t!("palette.hint"))
                    .desired_width(f32::INFINITY),
            );
            field.request_focus();
            if field.changed() {
                self.palette.selected = 0;
            }
            let (up, down, enter, escape) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::Enter),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            let count = entries.len();
            if count > 0 {
                if down {
                    self.palette.selected = (self.palette.selected + 1) % count;
                }
                if up {
                    self.palette.selected = (self.palette.selected + count - 1) % count;
                }
                self.palette.selected = self.palette.selected.min(count - 1);
            }
            ui.add_space(theme::SPACE_1);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (i, (entry, label)) in entries.iter().enumerate() {
                        let row = ui.selectable_label(i == self.palette.selected, label);
                        if i == self.palette.selected && (up || down) {
                            row.scroll_to_me(None);
                        }
                        if row.clicked() {
                            chosen = Some(*entry);
                        }
                    }
                    if entries.is_empty() {
                        ui.weak(t!("palette.none"));
                    }
                });
            if enter && let Some((entry, _)) = entries.get(self.palette.selected) {
                chosen = Some(*entry);
            }
            close = escape;
        });
        if chosen.is_some() || close {
            self.palette = State {
                pending: chosen,
                ..State::default()
            };
        }
    }

    /// The palette's choice that is not an action: run directly.
    pub(crate) fn run_palette_entry(&mut self, ctx: &egui::Context, entry: Entry) {
        match entry {
            Entry::Action(_) => {}
            Entry::Command(command) => self.run_timeline_command(ctx, command),
            Entry::Broll => {
                let count = self.insert_auto_broll();
                let message = if count == 0 {
                    t!("broll.none").into_owned()
                } else {
                    t!("broll.done", count = count).into_owned()
                };
                self.toast = Some((message, std::time::Instant::now()));
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/command_palette.rs"]
mod tests;
