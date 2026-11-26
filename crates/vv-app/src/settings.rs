//! User settings of the program (not of the project), saved in
//! `~/.config/tandem/settings.json`.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use vv_media::audio_file::AudioFileFormat;
use vv_media::proxy::ProxyQuality;

use crate::i18n::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    TogglePlayback,
    FastPlayback,
    StepBackward,
    StepForward,
    MarkIn,
    MarkOut,
    ClearIn,
    ClearOut,
    ClearInOut,
    FullscreenViewer,
    ViewerZoomFit,
    ViewerZoomActual,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    PasteAttributes,
    Delete,
    RippleDelete,
    Split,
    ToggleDisabled,
    RetimeControls,
    SelectAll,
    SelectFromPlayhead,
    Rename,
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    ImportMedia,
    Export,
    OpenSettings,
    ZoomIn,
    ZoomOut,
    AddMarker,
    SelectTool,
    SlipTool,
    OpenCompound,
    ExitCompound,
    DeleteLeft,
    DeleteRight,
    BladeTool,
    SelectLeftTool,
    SelectRightTool,
    Duplicate,
    ToggleMagnet,
    ToggleSnapping,
    TogglePreviewAxis,
    FitTimeline,
    MakeCompound,
    UngroupCompound,
    GoToStart,
    GoToEnd,
    PreviousEditPoint,
    NextEditPoint,
    StepBackward10,
    StepForward10,
    ShuttleBack,
    ShuttlePause,
    ShuttleForward,
    ExtractAudio,
    GroupClips,
    ToggleLinkage,
    InsertEdit,
    OverwriteEdit,
    AppendEdit,
    NudgeLeft,
    NudgeRight,
    NudgeLeft10,
    NudgeRight10,
    MatchFrame,
    CloseGaps,
    RippleTool,
    ReplaceEdit,
}

impl Action {
    pub const ALL: [Action; 75] = [
        Action::TogglePlayback,
        Action::FastPlayback,
        Action::StepBackward,
        Action::StepForward,
        Action::MarkIn,
        Action::MarkOut,
        Action::ClearIn,
        Action::ClearOut,
        Action::ClearInOut,
        Action::FullscreenViewer,
        Action::ViewerZoomFit,
        Action::ViewerZoomActual,
        Action::Undo,
        Action::Redo,
        Action::Copy,
        Action::Cut,
        Action::Paste,
        Action::PasteAttributes,
        Action::Delete,
        Action::RippleDelete,
        Action::Split,
        Action::ToggleDisabled,
        Action::RetimeControls,
        Action::SelectAll,
        Action::SelectFromPlayhead,
        Action::Rename,
        Action::NewProject,
        Action::OpenProject,
        Action::SaveProject,
        Action::SaveProjectAs,
        Action::ImportMedia,
        Action::Export,
        Action::OpenSettings,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::AddMarker,
        Action::SelectTool,
        Action::SlipTool,
        Action::OpenCompound,
        Action::ExitCompound,
        Action::DeleteLeft,
        Action::DeleteRight,
        Action::BladeTool,
        Action::SelectLeftTool,
        Action::SelectRightTool,
        Action::Duplicate,
        Action::ToggleMagnet,
        Action::ToggleSnapping,
        Action::TogglePreviewAxis,
        Action::FitTimeline,
        Action::MakeCompound,
        Action::UngroupCompound,
        Action::GoToStart,
        Action::GoToEnd,
        Action::PreviousEditPoint,
        Action::NextEditPoint,
        Action::StepBackward10,
        Action::StepForward10,
        Action::ShuttleBack,
        Action::ShuttlePause,
        Action::ShuttleForward,
        Action::ExtractAudio,
        Action::GroupClips,
        Action::ToggleLinkage,
        Action::InsertEdit,
        Action::OverwriteEdit,
        Action::AppendEdit,
        Action::NudgeLeft,
        Action::NudgeRight,
        Action::NudgeLeft10,
        Action::NudgeRight10,
        Action::MatchFrame,
        Action::CloseGaps,
        Action::RippleTool,
        Action::ReplaceEdit,
    ];

    /// Key in the settings file: must never be changed.
    pub fn id(self) -> &'static str {
        match self {
            Action::TogglePlayback => "toggle_playback",
            Action::FastPlayback => "fast_playback",
            Action::StepBackward => "step_backward",
            Action::StepForward => "step_forward",
            Action::MarkIn => "mark_in",
            Action::MarkOut => "mark_out",
            Action::ClearIn => "clear_in",
            Action::ClearOut => "clear_out",
            Action::ClearInOut => "clear_in_out",
            Action::FullscreenViewer => "fullscreen_viewer",
            Action::ViewerZoomFit => "viewer_zoom_fit",
            Action::ViewerZoomActual => "viewer_zoom_actual",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::Copy => "copy",
            Action::Cut => "cut",
            Action::Paste => "paste",
            Action::PasteAttributes => "paste_attributes",
            Action::Delete => "delete",
            Action::RippleDelete => "ripple_delete",
            Action::Split => "split",
            Action::ToggleDisabled => "toggle_disabled",
            Action::RetimeControls => "retime_controls",
            Action::SelectAll => "select_all",
            Action::SelectFromPlayhead => "select_from_playhead",
            Action::Rename => "rename",
            Action::NewProject => "new_project",
            Action::OpenProject => "open_project",
            Action::SaveProject => "save_project",
            Action::SaveProjectAs => "save_project_as",
            Action::ImportMedia => "import_media",
            Action::Export => "export",
            Action::OpenSettings => "open_settings",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::AddMarker => "add_marker",
            Action::SelectTool => "select_tool",
            Action::SlipTool => "slip_tool",
            Action::OpenCompound => "open_compound",
            Action::ExitCompound => "exit_compound",
            Action::DeleteLeft => "delete_left",
            Action::DeleteRight => "delete_right",
            Action::BladeTool => "blade_tool",
            Action::SelectLeftTool => "select_left_tool",
            Action::SelectRightTool => "select_right_tool",
            Action::Duplicate => "duplicate",
            Action::ToggleMagnet => "toggle_magnet",
            Action::ToggleSnapping => "toggle_snapping",
            Action::TogglePreviewAxis => "toggle_preview_axis",
            Action::FitTimeline => "fit_timeline",
            Action::MakeCompound => "make_compound",
            Action::UngroupCompound => "ungroup_compound",
            Action::GoToStart => "go_to_start",
            Action::GoToEnd => "go_to_end",
            Action::PreviousEditPoint => "previous_edit_point",
            Action::NextEditPoint => "next_edit_point",
            Action::StepBackward10 => "step_backward_10",
            Action::StepForward10 => "step_forward_10",
            Action::ShuttleBack => "shuttle_back",
            Action::ShuttlePause => "shuttle_pause",
            Action::ShuttleForward => "shuttle_forward",
            Action::ExtractAudio => "extract_audio",
            Action::GroupClips => "group_clips",
            Action::ToggleLinkage => "toggle_linkage",
            Action::InsertEdit => "insert_edit",
            Action::OverwriteEdit => "overwrite_edit",
            Action::AppendEdit => "append_edit",
            Action::NudgeLeft => "nudge_left",
            Action::NudgeRight => "nudge_right",
            Action::NudgeLeft10 => "nudge_left_10",
            Action::NudgeRight10 => "nudge_right_10",
            Action::MatchFrame => "match_frame",
            Action::CloseGaps => "close_gaps",
            Action::RippleTool => "ripple_tool",
            Action::ReplaceEdit => "replace_edit",
        }
    }

    pub fn label(self) -> Cow<'static, str> {
        t!(format!("action.{}", self.id()))
    }

    /// Returns the translated settings category for this action.
    ///
    /// The returned string is used to group shortcuts in the settings UI.
    pub fn category(self) -> Cow<'static, str> {
        match self {
            Action::TogglePlayback
            | Action::FastPlayback
            | Action::StepBackward
            | Action::StepForward
            | Action::MarkIn
            | Action::MarkOut
            | Action::ClearIn
            | Action::ClearOut
            | Action::ClearInOut
            | Action::FullscreenViewer
            | Action::ViewerZoomFit
            | Action::ViewerZoomActual
            | Action::GoToStart
            | Action::GoToEnd
            | Action::PreviousEditPoint
            | Action::NextEditPoint
            | Action::StepBackward10
            | Action::StepForward10
            | Action::ShuttleBack
            | Action::ShuttlePause
            | Action::ShuttleForward => t!("action_category.playback"),
            Action::Undo
            | Action::Redo
            | Action::Copy
            | Action::Cut
            | Action::Paste
            | Action::PasteAttributes
            | Action::Delete
            | Action::RippleDelete
            | Action::Split
            | Action::ToggleDisabled
            | Action::RetimeControls
            | Action::SelectAll
            | Action::SelectFromPlayhead
            | Action::Rename
            | Action::ReplaceEdit
            | Action::DeleteLeft
            | Action::DeleteRight
            | Action::Duplicate
            | Action::ExtractAudio
            | Action::GroupClips
            | Action::InsertEdit
            | Action::OverwriteEdit
            | Action::AppendEdit
            | Action::NudgeLeft
            | Action::NudgeRight
            | Action::NudgeLeft10
            | Action::NudgeRight10
            | Action::CloseGaps => t!("action_category.edit"),
            Action::NewProject
            | Action::OpenProject
            | Action::SaveProject
            | Action::SaveProjectAs
            | Action::ImportMedia
            | Action::Export
            | Action::OpenSettings => t!("action_category.file"),
            Action::ZoomIn
            | Action::ZoomOut
            | Action::AddMarker
            | Action::SelectTool
            | Action::SlipTool
            | Action::OpenCompound
            | Action::ExitCompound
            | Action::BladeTool
            | Action::SelectLeftTool
            | Action::SelectRightTool
            | Action::ToggleMagnet
            | Action::ToggleSnapping
            | Action::TogglePreviewAxis
            | Action::ToggleLinkage
            | Action::MatchFrame
            | Action::RippleTool
            | Action::FitTimeline
            | Action::MakeCompound
            | Action::UngroupCompound => {
                t!("action_category.timeline")
            }
        }
    }

    /// Returns the built-in keyboard shortcuts used when no custom binding exists.
    fn default_shortcuts(self) -> Vec<Shortcut> {
        use egui::Key;
        let plain = Shortcut::plain;
        let ctrl = Shortcut::ctrl;
        let ctrl_shift = |key| Shortcut {
            shift: true,
            ..Shortcut::ctrl(key)
        };
        match self {
            Action::TogglePlayback => vec![plain(Key::Space)],
            Action::FastPlayback => vec![],
            Action::StepBackward => vec![plain(Key::ArrowLeft)],
            Action::StepForward => vec![plain(Key::ArrowRight)],
            Action::MarkIn => vec![plain(Key::I)],
            Action::MarkOut => vec![plain(Key::O)],
            Action::ClearIn => vec![Shortcut {
                alt: true,
                ..plain(Key::I)
            }],
            Action::ClearOut => vec![Shortcut {
                alt: true,
                ..plain(Key::O)
            }],
            Action::ClearInOut => vec![Shortcut {
                alt: true,
                ..plain(Key::X)
            }],
            Action::FullscreenViewer => vec![ctrl_shift(Key::F)],
            Action::ViewerZoomFit => vec![Shortcut {
                alt: true,
                shift: true,
                ..plain(Key::Z)
            }],
            Action::ViewerZoomActual => vec![],
            Action::Undo => vec![ctrl(Key::Z)],
            Action::Redo => vec![ctrl_shift(Key::Z), ctrl(Key::Y)],
            Action::Copy => vec![ctrl(Key::C)],
            Action::Cut => vec![ctrl(Key::X)],
            Action::Paste => vec![ctrl(Key::V)],
            Action::PasteAttributes => vec![Shortcut {
                alt: true,
                ..plain(Key::V)
            }],
            Action::Delete => vec![plain(Key::Delete), plain(Key::Backspace)],
            // ISO key between left Shift and Z ("<" on Italian layouts).
            Action::RippleDelete => vec![plain(Key::IntlBackslash)],
            Action::Split => vec![ctrl(Key::B)],
            Action::ToggleDisabled => vec![plain(Key::V)],
            Action::RetimeControls => vec![ctrl(Key::R)],
            Action::SelectAll => vec![ctrl(Key::A)],
            Action::SelectFromPlayhead => vec![Shortcut {
                alt: true,
                ..plain(Key::Y)
            }],
            Action::Rename => vec![plain(Key::F2)],
            Action::NewProject => vec![ctrl(Key::N)],
            Action::OpenProject => vec![ctrl(Key::O)],
            Action::SaveProject => vec![ctrl(Key::S)],
            Action::SaveProjectAs => vec![Shortcut {
                alt: true,
                ..Shortcut::ctrl(Key::S)
            }],
            Action::ImportMedia => vec![ctrl(Key::I)],
            Action::Export => vec![ctrl(Key::E)],
            Action::OpenSettings => vec![ctrl(Key::Comma)],
            // "=" is the unshifted "+" of US layouts.
            Action::ZoomIn => vec![ctrl(Key::Plus), ctrl(Key::Equals)],
            Action::ZoomOut => vec![ctrl(Key::Minus)],
            Action::AddMarker => vec![plain(Key::M)],
            Action::SelectTool => vec![plain(Key::A)],
            Action::SlipTool => vec![Shortcut {
                shift: true,
                ..plain(Key::A)
            }],
            Action::OpenCompound => vec![plain(Key::Enter)],
            Action::ExitCompound => vec![Shortcut {
                alt: true,
                ..plain(Key::ArrowLeft)
            }],
            Action::DeleteLeft => vec![plain(Key::Q)],
            Action::DeleteRight => vec![plain(Key::W)],
            Action::BladeTool => vec![plain(Key::B)],
            Action::SelectLeftTool => vec![plain(Key::OpenBracket)],
            Action::SelectRightTool => vec![plain(Key::CloseBracket)],
            Action::Duplicate => vec![ctrl(Key::D)],
            Action::ToggleMagnet => vec![plain(Key::P)],
            Action::ToggleSnapping => vec![plain(Key::N)],
            Action::TogglePreviewAxis => vec![plain(Key::S)],
            // CapCut's "~" key.
            Action::ToggleLinkage => vec![plain(Key::Backtick)],
            Action::MakeCompound => vec![Shortcut {
                alt: true,
                ..plain(Key::G)
            }],
            Action::UngroupCompound => vec![Shortcut {
                alt: true,
                shift: true,
                ..plain(Key::G)
            }],
            Action::FitTimeline => vec![Shortcut {
                shift: true,
                ..plain(Key::Z)
            }],
            Action::GoToStart => vec![plain(Key::Home)],
            Action::GoToEnd => vec![plain(Key::End)],
            Action::PreviousEditPoint => vec![plain(Key::ArrowUp)],
            Action::NextEditPoint => vec![plain(Key::ArrowDown)],
            Action::StepBackward10 => vec![Shortcut {
                shift: true,
                ..plain(Key::ArrowLeft)
            }],
            Action::StepForward10 => vec![Shortcut {
                shift: true,
                ..plain(Key::ArrowRight)
            }],
            Action::ShuttleBack => vec![plain(Key::J)],
            Action::ShuttlePause => vec![plain(Key::K)],
            Action::ShuttleForward => vec![plain(Key::L)],
            Action::ExtractAudio => vec![ctrl_shift(Key::S)],
            Action::GroupClips => vec![ctrl(Key::G)],
            Action::InsertEdit => vec![plain(Key::Comma)],
            Action::OverwriteEdit => vec![plain(Key::Period)],
            Action::AppendEdit => vec![plain(Key::E)],
            Action::NudgeLeft => vec![ctrl(Key::ArrowLeft)],
            Action::NudgeRight => vec![ctrl(Key::ArrowRight)],
            Action::NudgeLeft10 => vec![ctrl_shift(Key::ArrowLeft)],
            Action::NudgeRight10 => vec![ctrl_shift(Key::ArrowRight)],
            Action::MatchFrame => vec![plain(Key::F)],
            Action::RippleTool => vec![plain(Key::R)],
            Action::ReplaceEdit => vec![Shortcut {
                shift: true,
                ..plain(Key::R)
            }],
            Action::CloseGaps => vec![Shortcut {
                alt: true,
                ..plain(Key::Backspace)
            }],
        }
    }

    fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// `ctrl` is Cmd on macOS (`Modifiers::command`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: egui::Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Shortcut {
    pub fn plain(key: egui::Key) -> Self {
        Self {
            key,
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub fn ctrl(key: egui::Key) -> Self {
        Self {
            ctrl: true,
            ..Self::plain(key)
        }
    }

    pub fn from_event(key: egui::Key, modifiers: egui::Modifiers) -> Self {
        Self {
            key,
            ctrl: modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }

    /// The shortcut a key press in the capture UI stands for. A modifier
    /// alone is not one: egui reports it as a key press of its own, which
    /// would end the capture before the rest of the combination.
    pub fn captured(event: &egui::Event) -> Option<Self> {
        use egui::Key::*;
        match event {
            egui::Event::Key {
                key:
                    ShiftLeft | ShiftRight | ControlLeft | ControlRight | AltLeft | AltRight | SuperLeft
                    | SuperRight,
                ..
            } => None,
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => Some(Self::from_event(*key, *modifiers)),
            egui::Event::Copy => Some(Self::ctrl(C)),
            egui::Event::Cut => Some(Self::ctrl(X)),
            egui::Event::Paste(_) => Some(Self::ctrl(V)),
            _ => None,
        }
    }

    fn modifiers(self) -> egui::Modifiers {
        egui::Modifiers {
            alt: self.alt,
            shift: self.shift,
            command: self.ctrl,
            ..Default::default()
        }
    }

    fn modifiers_match(self, current: egui::Modifiers) -> bool {
        // On symbols Shift may only be needed to produce the key ("+" on US
        // layouts): there it is ignored if the shortcut does not ask for it.
        if is_symbol(self.key) {
            current.matches_logically(self.modifiers()) && (self.alt || !current.alt)
        } else {
            current.matches_exact(self.modifiers())
        }
    }

    /// Ctrl+C/X/V arrive from eframe as `Copy`/`Cut`/`Paste` events,
    /// not as key presses.
    fn clipboard_event(self) -> Option<fn(&egui::Event) -> bool> {
        if !self.ctrl || self.shift || self.alt {
            return None;
        }
        match self.key {
            egui::Key::C => Some(|e| matches!(e, egui::Event::Copy)),
            egui::Key::X => Some(|e| matches!(e, egui::Event::Cut)),
            egui::Key::V => Some(|e| matches!(e, egui::Event::Paste(_))),
            _ => None,
        }
    }

    pub fn pressed(self, input: &egui::InputState) -> bool {
        if let Some(is_event) = self.clipboard_event()
            && input.events.iter().any(is_event)
        {
            return true;
        }
        input.key_pressed(self.key) && self.modifiers_match(input.modifiers)
    }

    pub fn down(self, input: &egui::InputState) -> bool {
        input.key_down(self.key) && self.modifiers_match(input.modifiers)
    }

    /// Settings file format, e.g. `Ctrl+Shift+S`.
    fn to_config(self) -> String {
        self.join(self.key.name())
    }

    fn from_config(text: &str) -> Option<Self> {
        let mut parts: Vec<&str> = text.split('+').collect();
        // "Ctrl++": the last "+" is the key, not a separator.
        let key_name = if text.ends_with("++") {
            parts.truncate(parts.len() - 2);
            "+"
        } else {
            parts.pop()?
        };
        let mut shortcut = Self::plain(egui::Key::from_name(key_name)?);
        for part in parts {
            match part {
                "Ctrl" => shortcut.ctrl = true,
                "Shift" => shortcut.shift = true,
                "Alt" => shortcut.alt = true,
                _ => return None,
            }
        }
        Some(shortcut)
    }

    fn join(self, key: &str) -> String {
        let mut text = String::new();
        for (on, name) in [
            (self.ctrl, "Ctrl+"),
            (self.shift, "Shift+"),
            (self.alt, "Alt+"),
        ] {
            if on {
                text.push_str(name);
            }
        }
        text.push_str(key);
        text
    }
}

impl Serialize for Shortcut {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_config())
    }
}

impl<'de> Deserialize<'de> for Shortcut {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_config(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown shortcut {text:?}")))
    }
}

impl std::fmt::Display for Shortcut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key: Cow<str> = match self.key {
            egui::Key::IntlBackslash => "<".into(),
            egui::Key::Minus => "-".into(),
            egui::Key::ArrowLeft => "⬅".into(),
            egui::Key::ArrowRight => "➡".into(),
            egui::Key::ArrowUp => "⬆".into(),
            egui::Key::ArrowDown => "⬇".into(),
            egui::Key::Space => t!("key.space"),
            egui::Key::Delete => t!("key.delete"),
            key => key.symbol_or_name().into(),
        };
        f.write_str(&self.join(&key))
    }
}

fn is_symbol(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        Plus | Minus
            | Equals
            | Colon
            | Semicolon
            | Comma
            | Period
            | Slash
            | Backslash
            | Pipe
            | Questionmark
            | Exclamationmark
            | Quote
            | Backtick
            | OpenBracket
            | CloseBracket
            | OpenCurlyBracket
            | CloseCurlyBracket
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<Action, Vec<Shortcut>>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: Action::ALL
                .into_iter()
                .map(|a| (a, a.default_shortcuts()))
                .collect(),
        }
    }
}

impl Keymap {
    pub fn shortcuts(&self, action: Action) -> &[Shortcut] {
        self.bindings.get(&action).map_or(&[], Vec::as_slice)
    }

    pub fn pressed(&self, action: Action, input: &egui::InputState) -> bool {
        self.shortcuts(action).iter().any(|s| s.pressed(input))
    }

    pub fn down(&self, action: Action, input: &egui::InputState) -> bool {
        self.shortcuts(action).iter().any(|s| s.down(input))
    }

    /// `"text (shortcut)"`, or just `"text"` if the action has none.
    pub fn menu_label(&self, text: &str, action: Action) -> String {
        match self.shortcuts(action).first() {
            Some(shortcut) => format!("{text} ({shortcut})"),
            None => text.to_owned(),
        }
    }

    /// Assigns `shortcut` to `action` (replacing `slot`, or in addition if
    /// `None`), removing it from any other action. Returns the actions it
    /// was taken away from.
    pub fn assign(
        &mut self,
        action: Action,
        slot: Option<usize>,
        shortcut: Shortcut,
    ) -> Vec<Action> {
        let mut stolen = Vec::new();
        for (&other, shortcuts) in &mut self.bindings {
            if other != action && shortcuts.contains(&shortcut) {
                shortcuts.retain(|s| *s != shortcut);
                stolen.push(other);
            }
        }
        let shortcuts = self.bindings.entry(action).or_default();
        match slot.filter(|&i| i < shortcuts.len()) {
            Some(i) => shortcuts[i] = shortcut,
            None => shortcuts.push(shortcut),
        }
        let mut seen = Vec::new();
        shortcuts.retain(|s| {
            let first = !seen.contains(s);
            seen.push(*s);
            first
        });
        stolen
    }

    pub fn remove(&mut self, action: Action, slot: usize) {
        if let Some(shortcuts) = self.bindings.get_mut(&action)
            && slot < shortcuts.len()
        {
            shortcuts.remove(slot);
        }
    }
}

const MAX_RECENT_PROJECTS: usize = 10;

/// Bumped when the defaults change wholesale: 2 is the CapCut layout, which
/// replaced the full dump of the old defaults saved by every earlier version.
const KEYMAP_VERSION: u32 = 2;

/// State of the UI panels (size, open/closed): saved to find the interface
/// as it was left on reopening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelLayout {
    pub media_pool_open: bool,
    pub effects_open: bool,
    /// What the Color editor's two scope slots show.
    pub color_scopes: [vv_render::ScopeKind; 2],
    /// Share of the left column's height given to the media pool when the
    /// Effects panel is open too.
    pub media_pool_fraction: f32,
    pub media_pool_layout: crate::media_pool::PoolLayout,
    pub library_layout: crate::media_pool::PoolLayout,
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            media_pool_open: true,
            effects_open: false,
            color_scopes: [
                vv_render::ScopeKind::Waveform,
                vv_render::ScopeKind::Vectorscope,
            ],
            media_pool_fraction: 0.5,
            media_pool_layout: Default::default(),
            library_layout: Default::default(),
        }
    }
}

/// Of `audio_budget_bytes`: 0, from a hand-edited file, would play the
/// timeline silent.
pub const AUDIO_BUDGET_MB: std::ops::RangeInclusive<u32> = 100..=32000;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub ai_provider: crate::ai_sequence::Provider,
    pub ai_model: String,
    pub ai_provider_models: BTreeMap<String, String>,
    pub ai_runtime: crate::ai_sequence::Runtime,
    pub download: crate::downloader::Prefs,
    pub ai_allow_api: bool,
    pub ai_free_only: bool,
    pub ai_free_key_identity: Option<String>,
    pub ai_limits: crate::ai_sequence::Limits,
    pub keymap: Keymap,
    pub language: Language,
    /// Kinetic scrolling (inertia after a touchpad swipe) on the timeline.
    pub kinetic_scroll: bool,
    pub kinetic_scroll_media_pool: bool,
    /// Preview from the all-intra proxy once ready: smooth scrubbing on
    /// long-GOP sources. Export always uses the originals.
    pub proxy_enabled: bool,
    /// With `proxy_enabled` off, proxies only for `heavy_to_decode` media.
    pub proxy_auto: bool,
    pub proxy_quality: ProxyQuality,
    pub preview_resolution: PreviewResolution,
    /// Seconds buffered ahead of/behind the playhead.
    pub lookahead_secs: f64,
    pub behind_secs: f64,
    /// At speeds above 1x, buffer only the frames playback shows.
    pub fast_playback_shown_frames_only: bool,
    /// A media the decoder cannot keep up with, above 1x or in a scrub,
    /// shows its keyframes meanwhile.
    pub keyframes_when_behind: bool,
    /// Memory budget for the decoded frame cache of every `RenderAhead`.
    pub cache_budget_bytes: usize,
    /// Memory budget for the timeline audio (`MixBufferCache`).
    pub audio_budget_bytes: usize,
    pub hw_decode: crate::hw_decode::HwDecodeMode,
    /// `None`: `hw_decode::default_budget_bytes`, which follows the RAM.
    pub hw_decode_budget_bytes: Option<usize>,
    /// Read at startup only, see `hw_decode::enable_intel_experimental_decode`.
    pub intel_experimental_decode: bool,
    /// Recently opened projects, most recent first.
    pub recent_projects: Vec<PathBuf>,
    pub panels: PanelLayout,
    /// Saved by the user, in the order they were first saved.
    pub compressor_presets: Vec<Preset<vv_core::MultibandCompressor>>,
    pub eq_presets: Vec<Preset<vv_core::Equalizer>>,
    /// The last VST3 scan; `None` until one has run, `Some` empty when it
    /// found none.
    pub vst3_plugins: Option<Vec<vv_plugin::PluginInfo>>,
    /// Bundles whose scan crashed; not loaded again until they change on disk.
    pub vst3_blocklist: Vec<Blocked>,
    /// Microphone of the voiceover, by name; `None` is the system's default.
    pub input_device: Option<String>,
    /// Of the takes; `None`, or one this FFmpeg lacks, is
    /// `AudioFileFormat::resolve`'s choice.
    pub recording_format: Option<AudioFileFormat>,
    /// Where the takes go; `None` is `Recordings` next to the project file.
    pub recording_dir: Option<PathBuf>,
    /// Speech, matting and voice models; `None` is `models` next to the library.
    pub models_dir: Option<PathBuf>,
    /// Folder of the last export, where the next one starts.
    pub last_export_dir: Option<PathBuf>,
    /// Serve MCP on a local socket for `vv-app mcp --attach`.
    pub mcp_enabled: bool,
    /// Where the Home lists the projects; `None` is `default_projects_dir`.
    pub projects_dir: Option<PathBuf>,
    /// The blocks' arrangement, as `crate::workspace::Workspace` writes it.
    pub workspace: Option<serde_json::Value>,
    /// The project being edited, cleared on a clean exit: still set at
    /// startup, the last run crashed with it open.
    pub open_project: Option<PathBuf>,
    /// Starred library items, see `library_ribbon::favorite_key`: the user's,
    /// valid in every project.
    pub favorites: Vec<String>,
    /// Text styles saved by the user from the text properties.
    pub text_templates: Vec<crate::library_ribbon::TextTemplate>,
    pub check_updates: bool,
    /// Tag the user chose to skip in the update prompt.
    pub ignored_update: Option<String>,
    pub shorts: ShortsPrefs,
}

/// Short-form workflow (auto zoom, beat sync, safe zones, layout, text
/// editing), remembered between sessions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortsPrefs {
    pub auto_zoom: vv_core::auto_zoom::Config,
    pub reframe: vv_core::reframe::Config,
    pub dub: crate::dub::Prefs,
    pub beat_snap: bool,
    /// Beats per clip of «Tagli sui beat»: 1, 2 or 4.
    pub beat_every: u8,
    pub safe_zones: bool,
    /// Id of a platform in `safe_zones.json`.
    pub safe_zone_platform: String,
    pub layout: crate::vertical_layout::LayoutMode,
    /// Offer the other layout when the window shape suits it better.
    pub propose_layout: bool,
    pub transcript: TranscriptPrefs,
}

impl Default for ShortsPrefs {
    fn default() -> Self {
        Self {
            auto_zoom: Default::default(),
            reframe: Default::default(),
            dub: Default::default(),
            beat_snap: true,
            beat_every: 1,
            safe_zones: false,
            safe_zone_platform: "tiktok".into(),
            layout: Default::default(),
            propose_layout: true,
            transcript: TranscriptPrefs::default(),
        }
    }
}

/// Cuts made from the transcript panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranscriptPrefs {
    /// Seconds of the pause kept on each side of a cut.
    pub margin: f64,
    /// Audio crossfade at each join, seconds (the mixer caps it at 20 ms).
    pub crossfade: f64,
    /// Shift the other tracks too (music, titles, subtitles).
    pub ripple_all: bool,
    pub show_pauses: bool,
}

impl Default for TranscriptPrefs {
    fn default() -> Self {
        Self {
            margin: 0.12,
            crossfade: 0.015,
            ripple_all: true,
            show_pauses: true,
        }
    }
}

/// Largest size the editor composes at, as a fraction of the timeline's
/// resolution. Export always composes at full resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreviewResolution {
    #[default]
    Full,
    Half,
    Quarter,
}

impl PreviewResolution {
    pub const ALL: [PreviewResolution; 3] = [Self::Full, Self::Half, Self::Quarter];

    /// Key in the settings file: must never be changed.
    pub fn id(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Half => "half",
            Self::Quarter => "quarter",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }

    pub fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset<P> {
    pub name: String,
    pub params: P,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ai_provider: Default::default(),
            ai_model: "gemini-4-hydrogen".into(),
            ai_provider_models: BTreeMap::new(),
            ai_runtime: Default::default(),
            download: Default::default(),
            ai_allow_api: false,
            ai_free_only: true,
            ai_free_key_identity: None,
            ai_limits: Default::default(),
            keymap: Keymap::default(),
            language: Language::default(),
            kinetic_scroll: true,
            kinetic_scroll_media_pool: true,
            proxy_enabled: false,
            proxy_auto: true,
            proxy_quality: ProxyQuality::default(),
            preview_resolution: PreviewResolution::default(),
            lookahead_secs: crate::render_ahead::DEFAULT_LOOKAHEAD_SECS,
            behind_secs: crate::render_ahead::DEFAULT_BEHIND_SECS,
            fast_playback_shown_frames_only: true,
            keyframes_when_behind: true,
            cache_budget_bytes: crate::DEFAULT_CACHE_BUDGET_BYTES,
            audio_budget_bytes: crate::mix_buffers::DEFAULT_AUDIO_BUDGET_BYTES,
            hw_decode: Default::default(),
            hw_decode_budget_bytes: None,
            intel_experimental_decode: false,
            recent_projects: Vec::new(),
            panels: PanelLayout::default(),
            compressor_presets: Vec::new(),
            eq_presets: Vec::new(),
            vst3_plugins: None,
            vst3_blocklist: Vec::new(),
            input_device: None,
            recording_format: None,
            recording_dir: None,
            models_dir: None,
            last_export_dir: None,
            mcp_enabled: false,
            projects_dir: None,
            workspace: None,
            open_project: None,
            favorites: Vec::new(),
            text_templates: Vec::new(),
            check_updates: true,
            ignored_update: None,
            shorts: ShortsPrefs::default(),
        }
    }
}

impl Settings {
    /// Quality of the proxies to use, `None` if they are off.
    pub fn proxy(&self) -> Option<ProxyQuality> {
        (self.proxy_enabled || self.proxy_auto).then_some(self.proxy_quality)
    }

    /// Whether `meta`'s media gets a proxy generated.
    pub fn wants_proxy(&self, meta: &vv_core::MediaMeta) -> bool {
        self.proxy_enabled || (self.proxy_auto && heavy_to_decode(meta))
    }

    pub fn hw_decode_budget_bytes(&self) -> usize {
        self.hw_decode_budget_bytes
            .unwrap_or_else(crate::hw_decode::default_budget_bytes)
    }

    pub fn projects_dir(&self) -> Option<PathBuf> {
        self.projects_dir.clone().or_else(default_projects_dir)
    }

    pub fn add_recent_project(&mut self, path: PathBuf) {
        self.recent_projects.retain(|p| p != &path);
        self.recent_projects.insert(0, path);
        self.recent_projects.truncate(MAX_RECENT_PROJECTS);
    }
}

#[derive(Serialize, Deserialize, Default)]
struct SettingsFile {
    ai_provider: Option<crate::ai_sequence::Provider>,
    ai_model: Option<String>,
    ai_provider_models: Option<BTreeMap<String, String>>,
    ai_runtime: Option<crate::ai_sequence::Runtime>,
    #[serde(default)]
    download: Option<crate::downloader::Prefs>,
    ai_allow_api: Option<bool>,
    ai_free_only: Option<bool>,
    ai_free_key_identity: Option<String>,
    ai_limits: Option<crate::ai_sequence::Limits>,
    /// `shortcuts` written against these defaults; the older ones are dropped.
    #[serde(default)]
    keymap_version: Option<u32>,
    /// Only the bindings that differ from the defaults.
    #[serde(default)]
    shortcuts: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    kinetic_scroll: Option<bool>,
    #[serde(default)]
    kinetic_scroll_media_pool: Option<bool>,
    /// Not `proxy_enabled`: that key was always saved as `true` when
    /// proxies were on by default, it does not reflect a user choice.
    #[serde(default)]
    use_proxies: Option<bool>,
    #[serde(default)]
    auto_proxies: Option<bool>,
    #[serde(default)]
    proxy_quality: Option<String>,
    #[serde(default)]
    preview_resolution: Option<String>,
    #[serde(default)]
    lookahead_secs: Option<f64>,
    #[serde(default)]
    behind_secs: Option<f64>,
    #[serde(default)]
    fast_playback_shown_frames_only: Option<bool>,
    #[serde(default)]
    keyframes_when_behind: Option<bool>,
    #[serde(default)]
    cache_budget_mb: Option<u32>,
    #[serde(default)]
    audio_budget_mb: Option<u32>,
    #[serde(default)]
    hw_decode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hw_decode_budget_mb: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    intel_experimental_decode: bool,
    #[serde(default)]
    recent_projects: Vec<PathBuf>,
    #[serde(default)]
    panels: PanelLayoutFile,
    /// One by one: a preset that no longer reads is dropped alone, not
    /// with the whole file.
    #[serde(default)]
    compressor_presets: Vec<serde_json::Value>,
    #[serde(default)]
    eq_presets: Vec<serde_json::Value>,
    #[serde(default)]
    vst3_plugins: Vec<serde_json::Value>,
    #[serde(default)]
    vst3_scanned: Option<bool>,
    #[serde(default)]
    vst3_blocklist: Vec<serde_json::Value>,
    #[serde(default)]
    input_device: Option<String>,
    #[serde(default)]
    recording_format: Option<String>,
    #[serde(default)]
    recording_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    models_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_export_dir: Option<PathBuf>,
    #[serde(default)]
    mcp_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projects_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    workspace: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open_project: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    favorites: Vec<String>,
    /// Read one by one: an unreadable template must not lose the settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    text_templates: Vec<serde_json::Value>,
    #[serde(default)]
    check_updates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignored_update: Option<String>,
    #[serde(default)]
    shorts: ShortsPrefs,
}

#[derive(Serialize, Deserialize, Default)]
struct PanelLayoutFile {
    #[serde(default)]
    media_pool_open: Option<bool>,
    #[serde(default)]
    effects_open: Option<bool>,
    #[serde(default)]
    color_scopes: Option<[vv_render::ScopeKind; 2]>,
    #[serde(default)]
    media_pool_fraction: Option<f32>,
    #[serde(default)]
    media_pool_layout: Option<crate::media_pool::PoolLayout>,
    #[serde(default)]
    library_layout: Option<crate::media_pool::PoolLayout>,
}

/// A bundle that crashed a scan, with its modification time (seconds since the
/// epoch) when it was blocked; `None` for entries saved before it was recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BlockedRepr")]
pub struct Blocked {
    pub path: PathBuf,
    pub modified: Option<u64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BlockedRepr {
    Path(PathBuf),
    Entry {
        path: PathBuf,
        modified: Option<u64>,
    },
}

impl From<BlockedRepr> for Blocked {
    fn from(repr: BlockedRepr) -> Self {
        match repr {
            BlockedRepr::Path(path) => Self {
                path,
                modified: None,
            },
            BlockedRepr::Entry { path, modified } => Self { path, modified },
        }
    }
}

impl Blocked {
    pub fn new(path: PathBuf) -> Self {
        let modified = bundle_mtime(&path);
        Self { path, modified }
    }

    /// Legacy entries (no recorded time) stay blocked.
    fn is_unchanged(&self) -> bool {
        self.modified.is_none() || self.modified == bundle_mtime(&self.path)
    }
}

/// Of the module binary (what crashed): replacing a bundle in place leaves the
/// directory's own time alone. Single-file bundles use their own.
fn bundle_mtime(path: &Path) -> Option<u64> {
    let file = vv_plugin::bundle_binary(path)
        .filter(|binary| binary.is_file())
        .unwrap_or_else(|| path.to_path_buf());
    let modified = std::fs::metadata(file).ok()?.modified().ok()?;
    Some(
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs(),
    )
}

impl Settings {
    /// A bundle that crashed the last scan is blocked (and so persisted) before
    /// this one starts. The blocklist only grows, except that a manual rescan
    /// retries bundles changed on disk since: clearing it all made several
    /// crashing bundles take turns crashing every rescan.
    pub fn prepare_plugin_scan(&mut self, manual: bool, marker: &Path) {
        if manual {
            self.vst3_blocklist.retain(Blocked::is_unchanged);
        }
        if let Some(crashed) = vv_plugin::take_crashed(marker)
            && !self.vst3_blocklist.iter().any(|b| b.path == crashed)
        {
            self.vst3_blocklist.push(Blocked::new(crashed));
        }
    }

    pub fn blocked_paths(&self) -> Vec<PathBuf> {
        self.vst3_blocklist.iter().map(|b| b.path.clone()).collect()
    }

    pub fn needs_plugin_scan(&self) -> bool {
        self.vst3_plugins.is_none()
    }

    pub fn default_path() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(config.join("tandem").join("settings.json"))
    }

    /// Missing or unreadable file = default settings; actions absent from
    /// the file keep unclaimed default shortcuts.
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let file: SettingsFile = match serde_json::from_str(&text) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("invalid settings in {}: {e}", path.display());
                return Self::default();
            }
        };
        let mut settings = Self::default();
        settings.ai_provider = file.ai_provider.unwrap_or_default();
        settings.ai_provider_models = file.ai_provider_models.unwrap_or_default();
        settings.ai_model = file.ai_model.unwrap_or_else(|| "gemini-4-hydrogen".into());
        settings.ai_runtime = file.ai_runtime.unwrap_or_default();
        settings.download = file.download.unwrap_or_default();
        settings.ai_allow_api = file.ai_allow_api.unwrap_or(false);
        settings.ai_free_only = file.ai_free_only.unwrap_or(true);
        settings.ai_free_key_identity = file.ai_free_key_identity;
        settings.ai_limits = file
            .ai_limits
            .filter(|limits| limits.validate().is_ok())
            .unwrap_or_default();
        settings.language = file
            .language
            .as_deref()
            .and_then(Language::from_id)
            .unwrap_or_default();
        settings.kinetic_scroll = file.kinetic_scroll.unwrap_or(true);
        settings.kinetic_scroll_media_pool = file.kinetic_scroll_media_pool.unwrap_or(true);
        settings.proxy_enabled = file.use_proxies.unwrap_or(settings.proxy_enabled);
        settings.proxy_auto = file.auto_proxies.unwrap_or(settings.proxy_auto);
        settings.proxy_quality = file
            .proxy_quality
            .as_deref()
            .and_then(ProxyQuality::from_id)
            .unwrap_or_default();
        settings.preview_resolution = file
            .preview_resolution
            .as_deref()
            .and_then(PreviewResolution::from_id)
            .unwrap_or_default();
        settings.lookahead_secs = file.lookahead_secs.unwrap_or(settings.lookahead_secs);
        settings.behind_secs = file.behind_secs.unwrap_or(settings.behind_secs);
        settings.fast_playback_shown_frames_only = file
            .fast_playback_shown_frames_only
            .unwrap_or(settings.fast_playback_shown_frames_only);
        settings.keyframes_when_behind = file
            .keyframes_when_behind
            .unwrap_or(settings.keyframes_when_behind);
        settings.cache_budget_bytes = file
            .cache_budget_mb
            .map_or(settings.cache_budget_bytes, |mb| mb as usize * 1_000_000);
        settings.audio_budget_bytes = file
            .audio_budget_mb
            .map_or(settings.audio_budget_bytes, |mb| {
                mb.clamp(*AUDIO_BUDGET_MB.start(), *AUDIO_BUDGET_MB.end()) as usize * 1_000_000
            });
        settings.hw_decode = file
            .hw_decode
            .as_deref()
            .and_then(crate::hw_decode::HwDecodeMode::from_id)
            .unwrap_or_default();
        settings.hw_decode_budget_bytes =
            file.hw_decode_budget_mb.map(|mb| mb as usize * 1_000_000);
        settings.intel_experimental_decode = file.intel_experimental_decode;
        settings.recent_projects = file.recent_projects;
        settings.input_device = file.input_device;
        settings.recording_format = file
            .recording_format
            .as_deref()
            .and_then(AudioFileFormat::from_id);
        settings.recording_dir = file.recording_dir;
        settings.models_dir = file.models_dir;
        settings.last_export_dir = file.last_export_dir;
        settings.compressor_presets = readable_entries(file.compressor_presets);
        settings.eq_presets = readable_entries(file.eq_presets);
        let scanned = file.vst3_scanned.unwrap_or(!file.vst3_plugins.is_empty());
        let plugins = readable_entries(file.vst3_plugins);
        settings.vst3_plugins = (scanned || !plugins.is_empty()).then_some(plugins);
        settings.vst3_blocklist = readable_entries(file.vst3_blocklist);
        settings.mcp_enabled = file.mcp_enabled.unwrap_or(false);
        settings.projects_dir = file.projects_dir;
        settings.workspace = file.workspace;
        settings.open_project = file.open_project;
        settings.favorites = file.favorites;
        settings.text_templates = file
            .text_templates
            .into_iter()
            .filter_map(|template| serde_json::from_value(template).ok())
            .collect();
        settings.check_updates = file.check_updates.unwrap_or(true);
        settings.ignored_update = file.ignored_update;
        settings.shorts = file.shorts;
        let defaults = PanelLayout::default();
        settings.panels = PanelLayout {
            media_pool_open: file
                .panels
                .media_pool_open
                .unwrap_or(defaults.media_pool_open),
            effects_open: file.panels.effects_open.unwrap_or(defaults.effects_open),
            color_scopes: file.panels.color_scopes.unwrap_or(defaults.color_scopes),
            media_pool_fraction: file
                .panels
                .media_pool_fraction
                .unwrap_or(defaults.media_pool_fraction),
            media_pool_layout: file.panels.media_pool_layout.unwrap_or_default(),
            library_layout: file.panels.library_layout.unwrap_or_default(),
        };
        let saved_shortcuts = if file.keymap_version == Some(KEYMAP_VERSION) {
            file.shortcuts
        } else {
            BTreeMap::new()
        };
        let saved_bindings: BTreeMap<_, _> = saved_shortcuts
            .into_iter()
            .filter_map(|(id, shortcuts)| {
                Action::from_id(&id).map(|action| {
                    let shortcuts = shortcuts
                        .iter()
                        .filter_map(|s| Shortcut::from_config(s))
                        .collect::<Vec<_>>();
                    (action, shortcuts)
                })
            })
            .collect();
        let saved_shortcuts: Vec<_> = saved_bindings.values().flatten().copied().collect();
        for (&action, shortcuts) in &saved_bindings {
            settings.keymap.bindings.insert(action, shortcuts.clone());
        }
        for action in Action::ALL {
            if saved_bindings.contains_key(&action) {
                continue;
            }
            if let Some(shortcuts) = settings.keymap.bindings.get_mut(&action) {
                shortcuts.retain(|shortcut| !saved_shortcuts.contains(shortcut));
            }
        }
        settings
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = SettingsFile {
            ai_provider: Some(self.ai_provider),
            ai_model: Some(self.ai_model.clone()),
            ai_provider_models: Some(self.ai_provider_models.clone()),
            ai_runtime: Some(self.ai_runtime.clone()),
            download: Some(self.download.clone()),
            ai_allow_api: Some(self.ai_allow_api),
            ai_free_only: Some(self.ai_free_only),
            ai_free_key_identity: self.ai_free_key_identity.clone(),
            ai_limits: Some(self.ai_limits.clone()),
            keymap_version: Some(KEYMAP_VERSION),
            shortcuts: self
                .keymap
                .bindings
                .iter()
                .filter(|(action, shortcuts)| **shortcuts != action.default_shortcuts())
                .map(|(action, shortcuts)| {
                    (
                        action.id().to_owned(),
                        shortcuts.iter().map(|s| s.to_config()).collect(),
                    )
                })
                .collect(),
            language: Some(self.language.id().to_owned()),
            kinetic_scroll: Some(self.kinetic_scroll),
            kinetic_scroll_media_pool: Some(self.kinetic_scroll_media_pool),
            use_proxies: Some(self.proxy_enabled),
            auto_proxies: Some(self.proxy_auto),
            proxy_quality: Some(self.proxy_quality.id().to_owned()),
            preview_resolution: Some(self.preview_resolution.id().to_owned()),
            lookahead_secs: Some(self.lookahead_secs),
            behind_secs: Some(self.behind_secs),
            fast_playback_shown_frames_only: Some(self.fast_playback_shown_frames_only),
            keyframes_when_behind: Some(self.keyframes_when_behind),
            cache_budget_mb: Some((self.cache_budget_bytes / 1_000_000) as u32),
            audio_budget_mb: Some((self.audio_budget_bytes / 1_000_000) as u32),
            hw_decode: Some(self.hw_decode.id().to_owned()),
            hw_decode_budget_mb: self
                .hw_decode_budget_bytes
                .map(|bytes| (bytes / 1_000_000) as u32),
            intel_experimental_decode: self.intel_experimental_decode,
            recent_projects: self.recent_projects.clone(),
            panels: PanelLayoutFile {
                media_pool_open: Some(self.panels.media_pool_open),
                effects_open: Some(self.panels.effects_open),
                color_scopes: Some(self.panels.color_scopes),
                media_pool_fraction: Some(self.panels.media_pool_fraction),
                media_pool_layout: Some(self.panels.media_pool_layout),
                library_layout: Some(self.panels.library_layout),
            },
            compressor_presets: entry_values(&self.compressor_presets),
            eq_presets: entry_values(&self.eq_presets),
            vst3_plugins: entry_values(self.vst3_plugins.as_deref().unwrap_or_default()),
            vst3_scanned: Some(self.vst3_plugins.is_some()),
            vst3_blocklist: entry_values(&self.vst3_blocklist),
            input_device: self.input_device.clone(),
            recording_format: self.recording_format.map(|f| f.id().to_owned()),
            recording_dir: self.recording_dir.clone(),
            models_dir: self.models_dir.clone(),
            last_export_dir: self.last_export_dir.clone(),
            mcp_enabled: Some(self.mcp_enabled),
            projects_dir: self.projects_dir.clone(),
            workspace: self.workspace.clone(),
            open_project: self.open_project.clone(),
            favorites: self.favorites.clone(),
            text_templates: self
                .text_templates
                .iter()
                .filter_map(|template| serde_json::to_value(template).ok())
                .collect(),
            check_updates: Some(self.check_updates),
            ignored_update: self.ignored_update.clone(),
            shorts: self.shorts.clone(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        vv_core::write_atomic(path, text.as_bytes()).map_err(|e| e.to_string())
    }
}

/// `~/Videos/Projects`.
pub fn default_projects_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join("Videos").join("Projects"))
}

fn readable_entries<T: serde::de::DeserializeOwned>(values: Vec<serde_json::Value>) -> Vec<T> {
    values
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect()
}

/// Entries that cannot be written as JSON (a path that is not valid UTF-8) are
/// dropped instead of failing the whole save.
fn entry_values<T: Serialize>(entries: &[T]) -> Vec<serde_json::Value> {
    let values: Vec<_> = entries
        .iter()
        .filter_map(|entry| serde_json::to_value(entry).ok())
        .collect();
    let dropped = entries.len() - values.len();
    if dropped > 0 {
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            eprintln!("settings: {dropped} entry(ies) that cannot be written are not saved");
        });
    }
    values
}

#[cfg(test)]
#[path = "tests/settings.rs"]
mod tests;

impl Settings {
    pub fn remember_ai_model_on_provider_change(&mut self, previous: crate::ai_sequence::Provider) {
        if previous == self.ai_provider {
            return;
        }
        self.ai_provider_models
            .insert(previous.id().into(), self.ai_model.clone());
        self.ai_model = self
            .ai_provider_models
            .get(self.ai_provider.id())
            .cloned()
            .unwrap_or_else(|| {
                if self.ai_provider == crate::ai_sequence::Provider::Gemini {
                    "gemini-4-hydrogen".into()
                } else {
                    String::new()
                }
            });
    }
}

impl Settings {
    pub fn sequence_api_access(&self) -> crate::ai_sequence::ApiAccess {
        use crate::ai_sequence::ApiAccess;
        let Ok(key) = crate::gemini::configured_key() else {
            return ApiAccess::Disabled;
        };
        let key_identity = crate::gemini::key_identity(&key);
        if self.ai_free_only {
            if self.ai_free_key_identity.as_deref() == Some(key_identity.as_str()) {
                ApiAccess::FreeProject { key_identity }
            } else {
                ApiAccess::Disabled
            }
        } else if self.ai_allow_api {
            ApiAccess::ConfiguredBilling { key_identity }
        } else {
            ApiAccess::Disabled
        }
    }
}

/// Media whose software decode cannot keep up with scrubbing on an ordinary
/// laptop: HEVC/AV1, or more pixels per second than 1080p30.
pub fn heavy_to_decode(meta: &vv_core::MediaMeta) -> bool {
    let codec = meta.file.video_codec.as_deref().unwrap_or_default();
    let pixel_rate = f64::from(meta.width) * f64::from(meta.height) * meta.fps.as_f64();
    matches!(codec, "hevc" | "av1") || pixel_rate > 1920.0 * 1088.0 * 30.0
}
