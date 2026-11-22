//! Persistent generation requests; all provider work happens outside the UI thread.
pub(crate) mod access;
pub use access::ApiAccess;
pub(crate) mod availability;
pub(crate) mod context;
pub(crate) mod insertion;
pub(crate) mod material;
pub(crate) mod portable;
pub(crate) mod process;
mod provider;
mod queue;
mod render;
pub(crate) mod revision;
pub(crate) mod setup;
pub use queue::Queue;
mod store;
pub(crate) mod suggestions;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Codex,
    Claude,
    OpenCode,
    #[default]
    Gemini,
}

impl Provider {
    pub const ALL: [Self; 4] = [Self::Codex, Self::Claude, Self::OpenCode, Self::Gemini];
    pub fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex CLI",
            Self::Claude => "Claude Code",
            Self::OpenCode => "OpenCode",
            Self::Gemini => "Gemini API",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::OpenCode => "opencode",
            Self::Gemini => "gemini",
        }
    }
    pub fn executable(self) -> Option<&'static str> {
        match self {
            Self::Codex => Some("codex"),
            Self::Claude => Some("claude"),
            Self::OpenCode => Some("opencode"),
            Self::Gemini => None,
        }
    }
}

/// Where the graphic sits in the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Fullscreen,
    Pill,
    AboveHead,
    Side,
}

impl Placement {
    /// Overlays must stay transparent so the speaker remains visible.
    pub fn is_overlay(self) -> bool {
        self != Self::Fullscreen
    }

    pub fn instructions(self) -> &'static str {
        match self {
            Self::Fullscreen => "Full-screen graphic: use the whole canvas.",
            Self::Pill => {
                "Pill overlay: one compact rounded capsule (fully rounded ends, about 70-85% of the canvas width, at most about 14% of its height) centered horizontally in the lower third, above the bottom 12% safe area. Keep text short and large. Everything outside the pill must be fully transparent so the video shows through."
            }
            Self::Side => {
                "Side-by-side layout: the graphic is a clean opaque panel with rounded corners and margins on the right 45% of the canvas, vertically centered, with large readable text. The left half of the canvas must stay fully transparent so the video shows through."
            }
            Self::AboveHead => {
                "Above-the-head overlay for a talking-head video: place the graphic in the empty space above the speaker's head, inside the top 30% of the canvas, centered on the speaker (use the attached frames to locate the head). Never cover the face. Everything else must be fully transparent so the speaker stays visible."
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Composition {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub frames: u32,
    pub transparent: bool,
}

impl Composition {
    pub fn validate(&self) -> Result<(), String> {
        if !(16..=7680).contains(&self.width)
            || !(16..=7680).contains(&self.height)
            || !self.fps.is_finite()
            || !(1.0..=120.0).contains(&self.fps)
            || self.frames == 0
            || f64::from(self.frames) / self.fps > 600.0
        {
            return Err(
                "Composition must be 16–7680 pixels, 1–120 fps and at most 10 minutes".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub prompt: String,
    pub context: String,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub placement: Placement,
    #[serde(default)]
    pub media: Vec<context::Source>,
    /// Spaces for the user's photos/videos known before generation.
    #[serde(default)]
    pub material: Vec<material::Slot>,
    /// The model may add spaces for material it should not invent.
    #[serde(default)]
    pub ask_material: bool,
    /// Re-renders the parent with new material, without calling the model.
    #[serde(default)]
    pub render_only: bool,
    pub provider: Provider,
    #[serde(skip)]
    pub api_access: ApiAccess,
    pub model: String,
    pub composition: Composition,
    #[serde(default)]
    pub limits: Limits,
    pub parent: Option<String>,
    #[serde(default = "reuse_session_default")]
    pub reuse_session: bool,
    pub insert: bool,
    /// The model picks the length; `composition.frames` is only the upper bound.
    #[serde(default)]
    pub auto_duration: bool,
    pub timeline: Option<u64>,
    pub anchor_clip: Option<u64>,
    #[serde(default)]
    pub anchor_details: Option<Anchor>,
    pub anchor_offset: i64,
    pub start: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anchor {
    pub source_frame: i64,
    pub rate: vv_core::Rational,
    pub media: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Queued,
    Preparing,
    Analyzing,
    Generating,
    Validating,
    Rendering,
    Completed,
    Failed,
    Cancelled,
}

impl Stage {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub request: Request,
    pub stage: Stage,
    pub error: Option<String>,
    pub attempt: u8,
    pub source: Option<String>,
    pub usage: Option<serde_json::Value>,
    #[serde(default)]
    pub usage_history: Vec<serde_json::Value>,
    pub imported: bool,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<context::Attachment>,
    /// Edits the model proposed for this exact composition.
    #[serde(default)]
    pub improvements: Vec<Improvement>,
    #[serde(default)]
    pub slots: Vec<material::Slot>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Improvement {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Sent as the revision prompt when the user picks the suggestion.
    pub instruction: String,
}

pub use store::Store;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Runtime {
    pub directory: PathBuf,
    pub node: PathBuf,
    pub browser: PathBuf,
}

#[cfg(test)]
#[path = "../tests/ai_sequence/mod.rs"]
mod tests;

fn reuse_session_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Limits {
    pub max_attempts: u8,
    pub provider_timeout_secs: u64,
    pub render_timeout_secs: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            provider_timeout_secs: 600,
            render_timeout_secs: 900,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=5).contains(&self.max_attempts)
            || !(10..=3600).contains(&self.provider_timeout_secs)
            || !(10..=3600).contains(&self.render_timeout_secs)
        {
            return Err(
                "AI jobs require 1–5 code attempts and timeouts between 10 and 3600 seconds".into(),
            );
        }
        Ok(())
    }
}

impl Stage {
    pub fn label(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Queued => t!("ai_sequence.stage_queued"),
            Self::Preparing => t!("ai_sequence.stage_preparing"),
            Self::Analyzing => t!("ai_sequence.stage_analyzing"),
            Self::Generating => t!("ai_sequence.stage_generating"),
            Self::Validating => t!("ai_sequence.stage_validating"),
            Self::Rendering => t!("ai_sequence.stage_rendering"),
            Self::Completed => t!("ai_sequence.stage_completed"),
            Self::Failed => t!("ai_sequence.stage_failed"),
            Self::Cancelled => t!("ai_sequence.stage_cancelled"),
        }
    }
}
