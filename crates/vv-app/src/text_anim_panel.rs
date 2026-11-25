//! Animation tab of a text clip: In / Out / Loop presets with an animated
//! preview on hover, their parameters, and karaoke.

use super::*;
use crate::properties_panel::{param_row, preview_combo, section_header, slider_field};
use vv_core::text_anim_presets as presets;
use vv_core::{
    AnimDescriptor, AnimState, Easing, Granularity, Karaoke, TextAnimations, TitleParams, UnitOrder,
};

const PREVIEW_HEIGHT: f32 = 90.0;
const HOLD_SECS: f32 = 0.6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    In,
    Out,
    Loop,
}

impl Slot {
    fn title(self) -> String {
        match self {
            Slot::In => t!("anim.in"),
            Slot::Out => t!("anim.out"),
            Slot::Loop => t!("anim.loop"),
        }
        .to_string()
    }

    fn ids(self) -> Vec<&'static str> {
        match self {
            Slot::In => presets::IN_IDS.to_vec(),
            Slot::Out => presets::out_ids(),
            Slot::Loop => presets::LOOP_IDS.to_vec(),
        }
    }

    fn preset(self, id: &str) -> Option<AnimDescriptor> {
        match self {
            Slot::In => presets::in_preset(id),
            Slot::Out => presets::out_preset(id),
            Slot::Loop => presets::loop_preset(id),
        }
    }

    fn get(self, anims: &mut TextAnimations) -> &mut Option<AnimDescriptor> {
        match self {
            Slot::In => &mut anims.enter,
            Slot::Out => &mut anims.exit,
            Slot::Loop => &mut anims.looped,
        }
    }
}

fn preset_label(id: &str) -> String {
    t!(format!("anim.preset.{id}")).into_owned()
}

/// The animation state `time` seconds into the looping preview.
fn preview_state(anims: &TextAnimations, text: &str, time: f32) -> Option<AnimState> {
    let longest = [&anims.enter, &anims.exit]
        .into_iter()
        .flatten()
        .map(|d| d.duration)
        .fold(0.0, f32::max);
    let len = if anims.looped.is_some() || anims.karaoke.is_some() {
        3.0
    } else if anims.enter.is_some() && anims.exit.is_some() {
        anims.enter.as_ref().map_or(0.0, |d| d.duration) + HOLD_SECS + longest
    } else {
        longest + HOLD_SECS
    };
    let t = time.rem_euclid(len + 0.4).min(len);
    anims.state_at(text, t, len)
}

fn preview(ui: &mut egui::Ui, title: &TitleParams, timeline_size: (u32, u32)) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PREVIEW_HEIGHT),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        rect,
        crate::theme::RADIUS_CONTROL,
        crate::theme::BG_INSET_ALT,
    );
    let time = ui.input(|i| i.time) as f32;
    let mut shown = title.clone();
    shown.anim_state = preview_state(&shown.anim, &shown.display_text(), time);
    let image = crate::library_ribbon::title_preview_image(&shown, timeline_size);
    let texture = ui
        .ctx()
        .load_texture("text_anim_preview", image, egui::TextureOptions::LINEAR);
    let aspect = texture.aspect_ratio();
    let size = if rect.width() / rect.height() > aspect {
        egui::vec2(rect.height() * aspect, rect.height())
    } else {
        egui::vec2(rect.width(), rect.width() / aspect)
    };
    ui.painter().image(
        texture.id(),
        egui::Rect::from_center_size(rect.center(), size),
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    ui.ctx().request_repaint();
}

fn slider_row(
    ui: &mut egui::Ui,
    label: String,
    value: &mut f32,
    default: f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    let row = param_row(ui, &label, None, |ui| {
        slider_field(ui, value, range, 0.01, 2)
    });
    if row.reset {
        *value = default;
    }
    row.changed || row.reset
}

fn parameters(ui: &mut egui::Ui, slot: Slot, d: &mut AnimDescriptor, id_salt: &str) -> bool {
    let default = slot.preset(&d.id).unwrap_or_default();
    let mut changed = false;
    let duration_label = if slot == Slot::Loop {
        t!("anim.period")
    } else {
        t!("anim.duration")
    };
    changed |= slider_row(
        ui,
        duration_label.to_string(),
        &mut d.duration,
        default.duration,
        0.1..=5.0,
    );
    changed |= slider_row(
        ui,
        t!("anim.stagger").to_string(),
        &mut d.stagger,
        default.stagger,
        0.0..=0.95,
    );

    let easings: Vec<_> = Easing::ALL
        .iter()
        .map(|e| (*e, crate::keyframe_editor::easing_label(*e), true))
        .collect();
    let grains: Vec<_> = Granularity::ALL
        .iter()
        .map(|g| (*g, t!(format!("anim.grain_{}", g.id())).into_owned(), true))
        .collect();
    let orders: Vec<_> = UnitOrder::ALL
        .iter()
        .map(|o| (*o, t!(format!("anim.order_{}", o.id())).into_owned(), true))
        .collect();
    if slot != Slot::Loop {
        let row = param_row(ui, &t!("anim.easing"), None, |ui| {
            preview_combo(
                ui,
                &format!("{id_salt}_ease"),
                &mut d.easing,
                &easings,
                None,
                Some(260.0),
                None,
            )
        });
        changed |= row.changed;
    }
    let row = param_row(ui, &t!("anim.granularity"), None, |ui| {
        preview_combo(
            ui,
            &format!("{id_salt}_grain"),
            &mut d.granularity,
            &grains,
            None,
            None,
            None,
        )
    });
    changed |= row.changed;
    let row = param_row(ui, &t!("anim.order"), None, |ui| {
        preview_combo(
            ui,
            &format!("{id_salt}_order"),
            &mut d.order,
            &orders,
            None,
            None,
            None,
        )
    });
    changed |= row.changed;
    changed
}

fn slot_section(
    ui: &mut egui::Ui,
    title: &mut TitleParams,
    slot: Slot,
    hovered: &mut Option<TextAnimations>,
) -> bool {
    let (open, reset) = section_header(ui, &slot.title(), true);
    let mut changed = false;
    if reset {
        *slot.get(&mut title.anim) = None;
        return true;
    }
    if !open {
        return false;
    }
    let current = slot.get(&mut title.anim).as_ref().map(|d| d.id.clone());
    let mut picked: Option<Option<String>> = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(current.is_none(), t!("anim.none"))
            .clicked()
        {
            picked = Some(None);
        }
        for id in slot.ids() {
            let response = ui.selectable_label(current.as_deref() == Some(id), preset_label(id));
            if response.hovered() {
                let mut trial = title.anim.clone();
                *slot.get(&mut trial) = slot.preset(id);
                *hovered = Some(trial);
            }
            if response.clicked() {
                picked = Some(Some(id.to_owned()));
            }
        }
    });
    if let Some(choice) = picked {
        *slot.get(&mut title.anim) = choice.and_then(|id| slot.preset(&id));
        changed = true;
    }
    let salt = format!("anim_{}", slot.title());
    if let Some(d) = slot.get(&mut title.anim) {
        changed |= parameters(ui, slot, d, &salt);
    }
    changed
}

fn karaoke_section(
    ui: &mut egui::Ui,
    title: &mut TitleParams,
    transcript_ms: Option<Vec<[u32; 2]>>,
) -> bool {
    let (open, reset) = section_header(ui, &t!("anim.karaoke"), true);
    if reset {
        title.anim.karaoke = None;
        return true;
    }
    if !open {
        return false;
    }
    let mut changed = false;
    let mut on = title.anim.karaoke.is_some();
    if ui.checkbox(&mut on, t!("anim.karaoke_on")).changed() {
        title.anim.karaoke = on.then(|| Karaoke {
            word_ms: transcript_ms.clone().unwrap_or_default(),
            ..presets::karaoke()
        });
        changed = true;
    }
    if let Some(k) = &mut title.anim.karaoke {
        let row = crate::properties_panel::color_row(ui, &t!("anim.karaoke_color"), &mut k.color);
        changed |= row.changed;
        changed |= slider_row(
            ui,
            t!("anim.karaoke_scale").to_string(),
            &mut k.scale,
            1.12,
            1.0..=1.6,
        );
        changed |= ui
            .checkbox(&mut k.keep_past, t!("anim.karaoke_keep"))
            .changed();
        let synced = transcript_ms.as_ref().is_some_and(|w| *w == k.word_ms);
        ui.add_enabled_ui(transcript_ms.is_some() && !synced, |ui| {
            if ui.button(t!("anim.karaoke_sync")).clicked() {
                k.word_ms = transcript_ms.clone().unwrap_or_default();
                changed = true;
            }
        });
        if transcript_ms.is_none() {
            ui.small(t!("anim.karaoke_even"));
        }
    }
    changed
}

/// Edits `title.anim`; `transcript_ms` are the words of the subtitles inside
/// the clip, when the clip is one. Returns whether something changed.
pub(crate) fn show(
    ui: &mut egui::Ui,
    title: &mut TitleParams,
    timeline_size: (u32, u32),
    transcript_ms: Option<Vec<[u32; 2]>>,
) -> bool {
    let memory = egui::Id::new("text_anim_hover");
    let previous: Option<TextAnimations> = ui.data_mut(|d| d.get_temp(memory));
    let mut shown = title.clone();
    if let Some(trial) = previous {
        shown.anim = trial;
    }
    preview(ui, &shown, timeline_size);
    ui.add_space(crate::theme::SPACE_2);

    let mut hovered = None;
    let mut changed = false;
    let slots_changed = [Slot::In, Slot::Out, Slot::Loop]
        .into_iter()
        .map(|slot| slot_section(ui, title, slot, &mut hovered))
        .fold(false, |a, b| a | b);
    changed |= slots_changed;
    changed |= karaoke_section(ui, title, transcript_ms);
    ui.data_mut(|d| {
        if let Some(trial) = hovered {
            d.insert_temp(memory, trial);
        } else {
            d.remove_temp::<TextAnimations>(memory);
        }
    });
    changed
}
