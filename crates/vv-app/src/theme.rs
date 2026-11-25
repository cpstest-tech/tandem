//! Single source of the interface look: colours, type, spacing, radii and
//! shadows. Values come from section 2 of `docs/design-spec.md`; new UI must
//! use these tokens only.

// Tokens of the spec not used by any widget yet.
#![allow(dead_code)]

use egui::{Color32, CornerRadius, FontId, Shadow, Stroke, TextStyle};

// Surfaces.
pub(crate) const BG_WINDOW: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
pub(crate) const BG_PANEL: Color32 = Color32::from_rgb(0x1c, 0x1c, 0x1c);
pub(crate) const BG_INSET: Color32 = Color32::from_rgb(0x17, 0x17, 0x17);
/// Alternate row of the track area (one step between window and inset).
pub(crate) const BG_INSET_ALT: Color32 = Color32::from_rgb(0x13, 0x13, 0x13);
pub(crate) const BG_RAISED: Color32 = Color32::from_rgb(0x2a, 0x2a, 0x2a);
pub(crate) const BG_RAISED_HOVER: Color32 = Color32::from_rgb(0x34, 0x34, 0x34);
pub(crate) const BG_SELECTED: Color32 = Color32::from_rgb(0x38, 0x38, 0x38);
pub(crate) const BG_MENU: Color32 = Color32::from_rgb(0x26, 0x26, 0x26);
pub(crate) const BORDER_SUBTLE: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2c);

// Text.
pub(crate) const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);
pub(crate) const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x9a, 0x9a, 0x9a);
/// Tertiary text (`.weak()`): metadata under a title, hints.
pub(crate) const TEXT_WEAK: Color32 = Color32::from_rgb(0x76, 0x76, 0x76);
pub(crate) const TEXT_DISABLED: Color32 = Color32::from_rgb(0x5c, 0x5c, 0x5c);

// Accent (cyan): active state, selection, primary action.
pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x1f, 0xc8, 0xd6);
/// Accent darkened enough to carry light text on top of it.
pub(crate) const ACCENT_FILL: Color32 = Color32::from_rgb(0x0f, 0x7c, 0x86);
/// Translucent accent for box selections.
pub(crate) const ACCENT_TRANSLUCENT: Color32 = Color32::from_rgba_premultiplied(5, 31, 34, 40);
/// Text on a solid accent button.
pub(crate) const ON_ACCENT: Color32 = Color32::WHITE;

// Timeline clips.
pub(crate) const CLIP_VIDEO: Color32 = Color32::from_rgb(0x0f, 0x6e, 0x73);
pub(crate) const CLIP_TEXT: Color32 = Color32::from_rgb(0xb5, 0x50, 0x3f);
pub(crate) const CLIP_AUDIO_FX: Color32 = Color32::from_rgb(0x2e, 0x4f, 0x87);
pub(crate) const CLIP_AUDIO_MUSIC: Color32 = Color32::from_rgb(0x12, 0x3c, 0x6e);
/// Not in the spec: effects/adjustment clips.
pub(crate) const CLIP_EFFECT: Color32 = Color32::from_rgb(0x5a, 0x4f, 0xa8);
pub(crate) const CLIP_SELECTED: Color32 = Color32::WHITE;

// Status.
pub(crate) const FAVORITE: Color32 = Color32::from_rgb(0xff, 0xc4, 0x00);
/// Errors: media offline, audio clipping.
pub(crate) const ERROR: Color32 = Color32::from_rgb(230, 70, 70);
pub(crate) const WARNING: Color32 = Color32::from_rgb(230, 200, 50);
pub(crate) const SUCCESS: Color32 = Color32::from_rgb(60, 200, 90);
pub(crate) const PLAYHEAD: Color32 = Color32::WHITE;

// Spacing (base 4).
pub(crate) const SPACE_1: f32 = 4.0;
pub(crate) const SPACE_2: f32 = 8.0;
pub(crate) const SPACE_3: f32 = 12.0;
pub(crate) const SPACE_4: f32 = 16.0;
pub(crate) const SPACE_5: f32 = 24.0;
pub(crate) const PANEL_GAP: f32 = 8.0;
pub(crate) const PANEL_PADDING: f32 = 10.0;

// Radii.
pub(crate) const RADIUS_TAG: f32 = 2.0;
pub(crate) const RADIUS_CLIP: f32 = 3.0;
pub(crate) const RADIUS_CONTROL: f32 = 4.0;
pub(crate) const RADIUS_PANEL: f32 = 6.0;
pub(crate) const RADIUS_MODAL: f32 = 8.0;

// Type sizes (px) and control heights.
/// Axis and scale labels inside graphs and meters.
pub(crate) const FONT_TINY: f32 = 10.0;
pub(crate) const FONT_SMALL: f32 = 11.0;
pub(crate) const FONT_BODY: f32 = 12.0;
pub(crate) const FONT_SECTION: f32 = 13.0;
pub(crate) const FONT_MODAL_TITLE: f32 = 14.0;
pub(crate) const FONT_LARGE: f32 = 16.0;
/// Banners and full-panel notices.
pub(crate) const FONT_DISPLAY: f32 = 20.0;
pub(crate) const CONTROL_HEIGHT: f32 = 24.0;
pub(crate) const FIELD_HEIGHT: f32 = 28.0;
pub(crate) const ICON_SIZE: f32 = 16.0;
pub(crate) const ICON_STROKE: f32 = 1.5;

// Shadows.
pub(crate) fn menu_shadow() -> Shadow {
    Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(128),
    }
}

pub(crate) fn panel_radius() -> CornerRadius {
    CornerRadius::same(RADIUS_PANEL as u8)
}

fn control_radius() -> CornerRadius {
    CornerRadius::same(RADIUS_CONTROL as u8)
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(0, "Inter".into());
    }
    ctx.set_fonts(fonts);
}

/// Control height of the touch density (vertical layout).
pub(crate) const TOUCH_CONTROL_HEIGHT: f32 = 36.0;

fn set_spacing(style: &mut egui::Style, touch: bool) {
    if touch {
        style.spacing.item_spacing = egui::vec2(SPACE_2 + 2.0, SPACE_2);
        style.spacing.button_padding = egui::vec2(SPACE_3, SPACE_2);
        style.spacing.interact_size.y = TOUCH_CONTROL_HEIGHT;
        style.spacing.icon_width = 20.0;
        style.spacing.icon_width_inner = 12.0;
        style.spacing.slider_rail_height = 10.0;
        style.interaction.resize_grab_radius_side = 10.0;
    } else {
        let default = egui::Style::default();
        style.spacing.item_spacing = egui::vec2(SPACE_2, SPACE_1 + 2.0);
        style.spacing.button_padding = egui::vec2(SPACE_2, SPACE_1 / 2.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
        style.spacing.icon_width = default.spacing.icon_width;
        style.spacing.icon_width_inner = default.spacing.icon_width_inner;
        style.spacing.slider_rail_height = default.spacing.slider_rail_height;
        style.interaction.resize_grab_radius_side = default.interaction.resize_grab_radius_side;
    }
}

/// Bigger targets for fingers, or back to the desktop sizes.
pub(crate) fn set_touch_density(ctx: &egui::Context, touch: bool) {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| set_spacing(style, touch));
    }
}

pub(crate) fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| {
            set_spacing(style, false);
            style.spacing.window_margin = egui::Margin::same(SPACE_4 as i8);
            style.spacing.menu_margin = egui::Margin::same(SPACE_1 as i8 + 2);
            style.spacing.scroll = egui::style::ScrollStyle::thin();
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(FONT_BODY));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(FONT_BODY));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(FONT_SMALL));
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(FONT_MODAL_TITLE));

            let v = &mut style.visuals;
            v.dark_mode = true;
            v.panel_fill = BG_PANEL;
            v.window_fill = BG_PANEL;
            v.window_stroke = Stroke::new(1.0, BORDER_SUBTLE);
            v.window_corner_radius = CornerRadius::same(RADIUS_MODAL as u8);
            v.menu_corner_radius = CornerRadius::same(RADIUS_PANEL as u8);
            v.window_shadow = menu_shadow();
            v.popup_shadow = menu_shadow();
            v.extreme_bg_color = BG_INSET;
            // Fields sit on bg-raised like every other control; inset made them vanish on panels.
            v.text_edit_bg_color = Some(BG_RAISED);
            v.weak_text_color = Some(TEXT_WEAK);
            v.slider_trailing_fill = true;
            v.faint_bg_color = BG_INSET;
            v.code_bg_color = BG_RAISED;
            v.hyperlink_color = ACCENT;
            v.warn_fg_color = WARNING;
            v.error_fg_color = ERROR;
            v.selection.bg_fill = ACCENT_FILL;
            v.selection.stroke = Stroke::new(1.0, ON_ACCENT);

            let w = &mut v.widgets;
            w.noninteractive.bg_fill = BG_PANEL;
            w.noninteractive.weak_bg_fill = BG_PANEL;
            w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_SUBTLE);
            w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);
            w.inactive.bg_fill = BG_RAISED;
            w.inactive.weak_bg_fill = BG_RAISED;
            w.inactive.bg_stroke = Stroke::NONE;
            w.inactive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
            w.hovered.bg_fill = BG_RAISED_HOVER;
            w.hovered.weak_bg_fill = BG_RAISED_HOVER;
            w.hovered.bg_stroke = Stroke::NONE;
            w.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            w.active.bg_fill = BG_SELECTED;
            w.active.weak_bg_fill = BG_SELECTED;
            w.active.bg_stroke = Stroke::NONE;
            w.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
            w.open.bg_fill = BG_SELECTED;
            w.open.weak_bg_fill = BG_SELECTED;
            w.open.bg_stroke = Stroke::NONE;
            w.open.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);
            for state in [
                &mut w.noninteractive,
                &mut w.inactive,
                &mut w.hovered,
                &mut w.active,
                &mut w.open,
            ] {
                state.corner_radius = control_radius();
                state.expansion = 0.0;
            }
        });
    }
}
