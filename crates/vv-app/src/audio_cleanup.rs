//! Noise reduction and automatic level: their settings, shown in the mixer's
//! effect window and in the Audio tab of Properties, which edits them on
//! the audio tracks of the selected clips.

use rust_i18n::t;
use vv_core::{
    AddAudioEffect, AudioEffect, AudioEffectKind, AutoLevel, CommandLabel, LevelSpeed,
    MixerChannel, MoveAudioEffect, NoiseStrength, SetAudioEffect, Timeline, TimelineId, TrackKind,
};

use crate::properties_panel::{BoxedCommand, param_row, section_header, slider_field};
use crate::{PanelTarget, VenturiApp};

fn strength_name(strength: NoiseStrength) -> String {
    match strength {
        NoiseStrength::Low => t!("mixer.strength_low"),
        NoiseStrength::Medium => t!("mixer.strength_medium"),
        NoiseStrength::High => t!("mixer.strength_high"),
    }
    .into_owned()
}

fn speed_name(speed: LevelSpeed) -> String {
    match speed {
        LevelSpeed::Slow => t!("mixer.speed_slow"),
        LevelSpeed::Medium => t!("mixer.speed_medium"),
        LevelSpeed::Fast => t!("mixer.speed_fast"),
    }
    .into_owned()
}

pub(crate) fn noise_reduction_settings(
    ui: &mut egui::Ui,
    mut strength: NoiseStrength,
) -> Option<AudioEffectKind> {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(t!("mixer.noise_strength"));
        for option in NoiseStrength::ALL {
            changed |= ui
                .selectable_value(&mut strength, option, strength_name(option))
                .changed();
        }
    });
    ui.small(t!("mixer.noise_hint"));
    changed.then_some(AudioEffectKind::NoiseReduction { strength })
}

pub(crate) fn auto_level_settings(
    ui: &mut egui::Ui,
    mut params: AutoLevel,
) -> Option<AudioEffectKind> {
    let mut changed = false;
    let (lo, hi) = AutoLevel::TARGET_RANGE;
    let row = param_row(ui, &t!("mixer.auto_level_target"), None, |ui| {
        slider_field(ui, &mut params.target_lufs, lo..=hi, 0.1, 1)
    });
    if row.reset {
        params.target_lufs = AutoLevel::DEFAULT.target_lufs;
    }
    changed |= row.changed || row.reset;
    let (lo, hi) = AutoLevel::MAX_GAIN_RANGE;
    let row = param_row(ui, &t!("mixer.auto_level_max_gain"), None, |ui| {
        slider_field(ui, &mut params.max_gain_db, lo..=hi, 0.1, 1)
    });
    if row.reset {
        params.max_gain_db = AutoLevel::DEFAULT.max_gain_db;
    }
    changed |= row.changed || row.reset;
    ui.horizontal(|ui| {
        ui.label(t!("mixer.auto_level_speed"));
        for option in LevelSpeed::ALL {
            changed |= ui
                .selectable_value(&mut params.speed, option, speed_name(option))
                .changed();
        }
    });
    ui.small(t!("mixer.auto_level_hint"));
    changed.then_some(AudioEffectKind::AutoLevel(params))
}

#[derive(Clone, Copy, PartialEq)]
enum Cleanup {
    Noise,
    Level,
}

impl Cleanup {
    fn matches(self, kind: &AudioEffectKind) -> bool {
        match self {
            Cleanup::Noise => matches!(kind, AudioEffectKind::NoiseReduction { .. }),
            Cleanup::Level => matches!(kind, AudioEffectKind::AutoLevel(_)),
        }
    }

    fn default_kind(self) -> AudioEffectKind {
        match self {
            Cleanup::Noise => AudioEffectKind::NoiseReduction {
                strength: NoiseStrength::default(),
            },
            Cleanup::Level => AudioEffectKind::AutoLevel(AutoLevel::DEFAULT),
        }
    }

    /// Where a new one goes: the noise first, so the level does not raise
    /// it; the level right after it, before the rest of the chain.
    fn insert_at(self, effects: &[AudioEffect]) -> usize {
        match self {
            Cleanup::Noise => 0,
            Cleanup::Level => effects
                .iter()
                .position(|e| !Cleanup::Noise.matches(&e.kind))
                .unwrap_or(effects.len()),
        }
    }
}

/// The audio tracks the clips play on: their own, or their linked audio's.
fn audio_tracks(timeline: &Timeline, targets: &[PanelTarget]) -> Vec<usize> {
    let mut tracks = Vec::new();
    let is_audio = |i: usize| {
        timeline
            .tracks
            .get(i)
            .is_some_and(|t| t.kind == TrackKind::Audio)
    };
    for target in targets {
        let found: Vec<usize> = if is_audio(target.track_index) {
            vec![target.track_index]
        } else {
            timeline
                .linked_members(target.track_index, target.clip_id)
                .into_iter()
                .map(|(track, _)| track)
                .filter(|&track| is_audio(track))
                .collect()
        };
        for track in found {
            if !tracks.contains(&track) {
                tracks.push(track);
            }
        }
    }
    tracks
}

fn commands_for(
    timeline_id: TimelineId,
    timeline: &Timeline,
    tracks: &[usize],
    cleanup: Cleanup,
    enabled: bool,
    kind: Option<&AudioEffectKind>,
) -> Vec<BoxedCommand> {
    let mut commands: Vec<BoxedCommand> = Vec::new();
    for &track in tracks {
        let channel = MixerChannel::Track(track);
        let effects = &timeline.tracks[track].mix.effects;
        match effects.iter().position(|e| cleanup.matches(&e.kind)) {
            Some(index) => {
                let effect = AudioEffect {
                    enabled,
                    kind: kind.cloned().unwrap_or_else(|| effects[index].kind.clone()),
                };
                if effect != effects[index] {
                    commands.push(Box::new(SetAudioEffect::new(
                        timeline_id,
                        channel,
                        index,
                        effect,
                        CommandLabel::EditAudioEffect,
                    )));
                }
            }
            None if enabled => {
                let kind = kind.cloned().unwrap_or_else(|| cleanup.default_kind());
                commands.push(Box::new(AddAudioEffect::new(
                    timeline_id,
                    channel,
                    AudioEffect::new(kind),
                )));
                let to = cleanup.insert_at(effects);
                if to < effects.len() {
                    commands.push(Box::new(MoveAudioEffect::new(
                        timeline_id,
                        channel,
                        effects.len(),
                        to,
                    )));
                }
            }
            None => {}
        }
    }
    commands
}

impl VenturiApp {
    /// Noise reduction and automatic level of the Audio tab. They are effects
    /// of the track (the mixer shows them too): a clip has no chain of its own.
    pub(crate) fn show_audio_cleanup(
        &self,
        ui: &mut egui::Ui,
        targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let tracks = audio_tracks(timeline, targets);
        for cleanup in [Cleanup::Noise, Cleanup::Level] {
            let title = match cleanup {
                Cleanup::Noise => t!("props.noise_reduction"),
                Cleanup::Level => t!("props.auto_level"),
            };
            ui.add_space(crate::theme::SPACE_2);
            let (open, reset) = section_header(ui, &title, true);
            if tracks.is_empty() {
                if open {
                    ui.weak(t!("props.audio_cleanup_no_track"));
                }
                continue;
            }
            let found = tracks.first().and_then(|&track| {
                timeline.tracks[track]
                    .mix
                    .effects
                    .iter()
                    .find(|e| cleanup.matches(&e.kind))
            });
            let was_on = found.is_some_and(|e| e.enabled);
            let kind = found.map_or_else(|| cleanup.default_kind(), |e| e.kind.clone());
            if reset {
                let default = cleanup.default_kind();
                pending.extend(commands_for(
                    timeline_id,
                    timeline,
                    &tracks,
                    cleanup,
                    was_on,
                    Some(&default),
                ));
                continue;
            }
            if !open {
                continue;
            }
            let mut on = was_on;
            if ui.checkbox(&mut on, t!("props.audio_cleanup_on")).changed() {
                pending.extend(commands_for(
                    timeline_id,
                    timeline,
                    &tracks,
                    cleanup,
                    on,
                    None,
                ));
            }
            let edited = ui
                .add_enabled_ui(on, |ui| match &kind {
                    &AudioEffectKind::NoiseReduction { strength } => {
                        noise_reduction_settings(ui, strength)
                    }
                    AudioEffectKind::AutoLevel(params) => auto_level_settings(ui, *params),
                    _ => None,
                })
                .inner;
            if let Some(kind) = edited {
                pending.extend(commands_for(
                    timeline_id,
                    timeline,
                    &tracks,
                    cleanup,
                    true,
                    Some(&kind),
                ));
            }
            let names: Vec<String> = tracks.iter().map(|&t| timeline.track_label(t)).collect();
            ui.small(t!("props.audio_cleanup_tracks", tracks = names.join(", ")));
        }
    }
}

#[cfg(test)]
#[path = "tests/audio_cleanup.rs"]
mod tests;
