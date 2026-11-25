//! Shared building blocks on top of the `theme` tokens, so titles, buttons and
//! status messages look the same in every panel and dialog.

use egui::RichText;

use crate::theme;

/// Title at the top of a dialog or full-screen page.
pub(crate) fn heading(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(
        RichText::new(text)
            .size(theme::FONT_MODAL_TITLE)
            .color(theme::TEXT_PRIMARY),
    )
}

/// Title of a group of controls inside a panel or dialog.
pub(crate) fn section_title(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .size(theme::FONT_SECTION)
        .color(theme::TEXT_PRIMARY)
}

/// Secondary line under a title: hints, metadata.
pub(crate) fn hint(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .size(theme::FONT_SMALL)
        .color(theme::TEXT_SECONDARY)
}

/// The one solid-accent button of a view (Export, Apply, Start…).
pub(crate) fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text)
            .size(theme::FONT_BODY)
            .color(theme::ON_ACCENT),
    )
    .fill(theme::ACCENT_FILL)
    .corner_radius(theme::RADIUS_CONTROL)
    .min_size(egui::vec2(0.0, theme::CONTROL_HEIGHT))
}

#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Error,
    Warning,
    Success,
}

pub(crate) fn status(ui: &mut egui::Ui, tone: Tone, text: impl Into<String>) -> egui::Response {
    let color = match tone {
        Tone::Error => theme::ERROR,
        Tone::Warning => theme::WARNING,
        Tone::Success => theme::SUCCESS,
    };
    ui.colored_label(color, RichText::new(text))
}
