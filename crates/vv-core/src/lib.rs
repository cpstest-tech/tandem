pub mod auto_cut;
pub mod auto_cut_edit;
pub mod auto_zoom;
pub mod beat_sync;
pub mod captions;
pub mod clip_anim;
pub mod clipboard;
pub mod command;
pub mod dub;
pub mod easing;
pub mod edit;
pub mod grade;
pub mod id_map;
mod legacy_slotmap;
pub mod lut;
pub mod mask;
pub mod model;
pub mod otio;
pub mod persistence;
pub mod pool;
pub mod reframe;
pub mod speed_curve;
mod stable_hash;
pub mod style_pack;
pub mod text_anim;
pub mod text_anim_presets;
pub mod text_style;
pub mod text_template;
pub mod transcript_edit;
pub mod workflow;

pub use captions::{CaptionWord, Captions, SetCaptions, SetSubtitlesLinked, SubtitleTemplate};
pub use command::{
    AddAudioEffect, AddTrack, Command, CommandLabel, CompositeCommand, CompoundPlan, FadeEdge,
    GroupMark, History, InsertClip, JoinableStep, KeyframePick, KeyframeTarget, KeyframeValue,
    LiftDelete, LinkClips, MixerChannel, MixerParam, MoveAudioEffect, MoveClips, MoveKeyframes,
    RemoveAudioEffect, RemoveKeyframe, RemoveMedia, RemoveTrack, ResetTransformParams,
    RippleDeleteGap, SetAiSequence, SetAudioEffect, SetClipAttributes, SetClipColor, SetClipFade,
    SetClipFreeze, SetClipSpeed, SetClipValue, SetClipsDisabled, SetClipsDisplayColor,
    SetClipsReversed, SetCover, SetCrossTransition, SetKeyframeInterpolation, SetMarker,
    SetMediaPath, SetMixerParam, SetProcessingPrecision, SetTimelineFps, SetTimelineResolution,
    SetTrackFlag, SlipClip, SpeedFit, SplitClip, TrackFlag, TrimClip, TrimEdge, UnlinkClip,
    UpsertKeyframe, compound_clip_commands, cut_overlaps, insert_overwriting, make_room_for_ranges,
    plan_compound_clip, reset_clip_gain, set_clip_assets, set_clip_blend_mode, set_clip_canvas,
    set_clip_filters, set_clip_flip, set_clip_gain, set_clip_masks, set_clip_matte,
    set_clip_remove_background, set_clip_title, set_clip_transform_param, set_clip_transition,
};
pub use easing::Easing;
pub use grade::*;
pub use mask::*;
pub use speed_curve::{CurvePreset, SetClipSpeedCurve, SpeedCurve};
pub mod fx;
pub use model::*;
pub use otio::{
    MeasureTitle, OtioError, OtioImport, OtioWarning, TitleMetrics, export_otio, import_otio,
    media_url_count, project_from_otio,
};
pub use persistence::{
    CURRENT_FORMAT_VERSION, PersistenceError, load_project, project_to_string, save_project,
    write_atomic,
};
pub use pool::{
    AddEntities, DeleteFolder, MoveFolder, RenameFolder, RenameTimeline, SetMediaFolder,
};
pub use stable_hash::{StableHasher, stable_hash};
pub use text_anim::*;
pub use text_style::*;
pub use text_template::{TemplateClip, TextTemplateDef};

#[cfg(test)]
#[path = "tests/test_support.rs"]
mod test_support;

#[cfg(test)]
#[path = "tests/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/lib_compound_clip.rs"]
mod compound_clip_tests;

#[cfg(test)]
#[path = "tests/lib_clip_speed.rs"]
mod clip_speed_tests;

#[cfg(test)]
#[path = "tests/workflow.rs"]
mod workflow_tests;
