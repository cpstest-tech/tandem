// Release builds only: a debug build keeps the console for the logs.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! egui window: media pool, viewer, properties panel, timeline. The
//! viewer composes on the GPU the active clips of all the video tracks and passes
//! the texture to egui-wgpu without readback (plans/REFACTOR_PIPELINE.md B2).

#[macro_use]
extern crate rust_i18n;
rust_i18n::i18n!("locales", fallback = "en");

mod adjust_tab;
mod ai_clips;
mod ai_context;
mod ai_sequence;
mod ai_sequence_cards;
mod ai_sequence_ui;
mod app_menu;
mod audio_cleanup;
mod audio_sync;
mod auto_cut;
mod auto_cut_ui;
mod auto_zoom;
mod autosave;
mod beat_sync;
mod canvas_ui;
mod color_window;
mod compressor_panel;
mod cover;
mod downloader;
mod downloader_ui;
mod dub;
mod edit_actions;
mod eq_panel;
mod export_dialog;
mod export_modal;
mod export_plan;
mod first_media;
mod fonts;
mod forced_relink_dialog;
mod fx_library;
mod gemini;
mod grade_panel;
mod home;
mod hw_decode;
mod i18n;
mod keyframe_editor;
mod library;
mod library_copy;
mod library_ribbon;
mod library_ui;
mod mask_overlay;
mod mask_panel;
mod matte_worker;
mod mcp_host;
mod media_pool;
mod media_pool_ui;
mod menu_list;
mod mix_buffers;
mod mixer_panel;
mod new_timeline_dialog;
mod paste_attributes;
mod paste_image;
mod plugin_editors;
mod pool_view;
mod presets;
mod project_details;
mod project_io;
mod projects;
mod properties_panel;
mod properties_tabs;
mod proxy_worker;
mod reframe;
mod render_ahead;
mod ribbon_editors;
mod safe_zones;
mod settings;
mod settings_dialog;
mod silence_dialog;
mod slip_viewer;
mod spawn;
mod speed_curve_ui;
mod speed_dialog;
mod stabilize_ui;
mod sticker_anim;
mod stickers;
mod stickers_ui;
mod style_packs;
mod style_packs_ui;
mod subtitles;
mod subtitles_ui;
mod text_anim_panel;
mod text_prewarm;
mod text_style_panel;
mod text_templates;
mod theme;
mod thumbnail_worker;
mod timeline_audio;
mod timeline_bar;
mod timeline_commands;
mod timeline_ui;
mod tracking_ui;
mod transcript;
mod transcript_ui;
mod transport;
mod updater;
mod updater_ui;
mod vertical_layout;
mod viewer_overlay;
mod viewer_waveform;
mod viewer_zoom;
mod voice_ui;
mod voiceover;
mod waveform_worker;
#[cfg(target_os = "linux")]
mod wayland_dnd;
#[cfg(target_os = "linux")]
mod wayland_gestures;
mod workspace;

use eframe::wgpu;
use media_pool_ui::*;
use new_timeline_dialog::NewTimelineDialog;
use paste_attributes::PasteAttributesDialog;
use project_io::*;
use properties_panel::*;
use settings::Action;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use timeline_audio::TimelineAudio;
use vv_core::edit::TargetTracks;
use vv_core::{ClipId, FolderId, FrameIdx, MediaId, TimelineId, Track, TrackKind};
use vv_session::{export, forced_relink, frame_provider, relink_job, worker};
use workspace::Block;

/// ~6 s of margin at 1080p, ~1.5 s at 4K.
const DEFAULT_CACHE_BUDGET_BYTES: usize = 1_200_000_000;

/// Floor of the shrunk lookahead: below this playback stutters anyway.
const MIN_LOOKAHEAD_SECS: f64 = 0.5;

/// After how long (s) an arrow held down stops doing the single
/// step and starts scrolling at `ARROW_HOLD_SPEED`.
const ARROW_HOLD_DELAY_SECS: f64 = 0.3;
const ARROW_HOLD_SPEED: f64 = 0.5;

/// Left/right arrow held down (`VenturiApp::arrow_hold`).
struct ArrowHold {
    direction: FrameIdx,
    pressed_at: f64,
    start_frame: FrameIdx,
}

struct MarkTarget<'a> {
    marks: &'a mut transport::MarkRange,
    total: FrameIdx,
    playhead: FrameIdx,
}

/// Playhead position with an arrow held for `elapsed` seconds:
/// one frame immediately, then continuous scrolling after `ARROW_HOLD_DELAY_SECS`.
fn arrow_hold_target(hold: &ArrowHold, elapsed: f64, fps: f64) -> FrameIdx {
    let scrolled =
        ((elapsed - ARROW_HOLD_DELAY_SECS).max(0.0) * fps * ARROW_HOLD_SPEED) as FrameIdx;
    (hold.start_frame + hold.direction * (1 + scrolled)).max(0)
}

/// The two fast playback speeds reachable with the "a" key
/// (see `VenturiApp::handle_fast_playback_key`) — not a free `f64`:
/// only these two factors are ever requested of `vv_audio::stretch_samples`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpeedTier {
    X2,
    X4,
    X8,
}

impl SpeedTier {
    fn tempo(self) -> f64 {
        match self {
            SpeedTier::X2 => 2.0,
            SpeedTier::X4 => 4.0,
            SpeedTier::X8 => 8.0,
        }
    }

    /// The tier reached by pressing "a" one more time relative to this one —
    /// see `VenturiApp::handle_fast_playback_key`. It stays at X8 past that.
    fn next(self) -> Self {
        match self {
            SpeedTier::X2 => SpeedTier::X4,
            SpeedTier::X4 | SpeedTier::X8 => SpeedTier::X8,
        }
    }

    fn from_multiplier(speed: f64) -> Option<Self> {
        if (speed - 2.0).abs() < 1e-9 {
            Some(SpeedTier::X2)
        } else if (speed - 4.0).abs() < 1e-9 {
            Some(SpeedTier::X4)
        } else if (speed - 8.0).abs() < 1e-9 {
            Some(SpeedTier::X8)
        } else {
            None
        }
    }
}

/// A selected clip, target of the properties panel. `source_frame`
/// is the playhead in the source space of *this* clip.
#[derive(Debug, Clone, Copy)]
struct PanelTarget {
    timeline: TimelineId,
    track_index: usize,
    clip_id: ClipId,
    source_frame: FrameIdx,
    timeline_start: FrameIdx,
    is_solid_color: bool,
    is_text: bool,
}

fn panel_target(
    timeline: TimelineId,
    track_index: usize,
    clip: &vv_core::Clip,
    playhead: FrameIdx,
) -> PanelTarget {
    let local = (playhead - clip.timeline_start).clamp(0, clip.timeline_len.saturating_sub(1));
    PanelTarget {
        timeline,
        track_index,
        clip_id: clip.id,
        source_frame: clip.source_frame_at(clip.timeline_start + local),
        timeline_start: clip.timeline_start,
        is_solid_color: matches!(clip.source, vv_core::ClipSource::SolidColor),
        is_text: matches!(clip.source, vv_core::ClipSource::Text),
    }
}

/// Which clips the properties panel edits: a video clip or its audio part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PropertiesTab {
    Video,
    Audio,
}

/// System fonts for the Title panel, read the first time they are needed.
#[derive(Default)]
struct FontCatalog {
    families: Option<Vec<String>>,
    faces: HashMap<String, Vec<vv_render::text::FontFace>>,
}

impl FontCatalog {
    fn families(&mut self) -> &[String] {
        self.families
            .get_or_insert_with(vv_render::text::font_families)
    }

    fn faces(&mut self, family: &str) -> &[vv_render::text::FontFace] {
        self.faces
            .entry(family.to_owned())
            .or_insert_with(|| vv_render::text::font_faces(family))
    }
}

#[derive(Debug, Clone)]
struct ClipPanelInfo {
    is_solid_color: bool,
    /// Native resolution of the clip's media and resolution of the
    /// timeline: the units of the pixel parameters of the `Transform` (the crop
    /// in the former, position and anchor in the latter).
    source_size: (u32, u32),
    timeline_size: (u32, u32),
    /// Keyframe state of every parameter, indexed by
    /// `TransformParam::index`.
    params: Vec<RowKeyframe>,
    transform: vv_core::Transform,
    gain_kf_here: bool,
    gain: f32,
    /// Nearest gain keyframes before/after, in source frames: the navigation
    /// arrows of the Volume row.
    gain_prev: Option<FrameIdx>,
    gain_next: Option<FrameIdx>,
    color_kf_here: bool,
    color: vv_core::Rgba,
    /// Taken `at` the panel's frame.
    title: Option<vv_core::TitleParams>,
    text_key: properties_panel::RowKeyframe,
    offset_key: properties_panel::RowKeyframe,
    /// Empty until a filter is dragged onto the clip from the Effects
    /// panel; then one per filter, in order of application.
    filters: Vec<FilterPanelInfo>,
    blend_mode: vv_core::BlendMode,
    matte: Option<vv_core::MatteMode>,
    /// A catalog sticker made to be recolored, and the color it has now.
    tintable: bool,
    tint: Option<vv_core::Rgba>,
}

/// A filter of `ClipPanelInfo`, its parameters evaluated at the panel's frame.
#[derive(Debug, Clone)]
struct FilterPanelInfo {
    kind: vv_core::FilterKind,
    enabled: bool,
    radius: f32,
    radius_key: RowKeyframe,
    direction: vv_core::BlurDirection,
    direction_key: RowKeyframe,
    amount: f32,
    amount_key: RowKeyframe,
    grade: grade_panel::GradeInfo,
    offset: [f32; 2],
    /// Of both axes, edited in one row.
    offset_key: RowKeyframe,
    grow: f32,
    grow_key: RowKeyframe,
    color: vv_core::Rgba,
}

/// A clip's track composed by itself, for the auto balance: a
/// picture-in-picture or a title over the clip would skew it. An adjustment
/// layer keeps the tracks below, which it works on.
#[derive(Debug, Clone, Copy)]
struct Isolation {
    track_index: usize,
    keep_below: bool,
}

impl Isolation {
    fn shows(self, track_index: usize) -> bool {
        track_index == self.track_index || (self.keep_below && track_index < self.track_index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewerFrameKind {
    Video,
    /// The clip under the playhead points at a media no longer in the media pool.
    Offline,
}

/// What the viewer browses instead of the timeline.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Browsed {
    Media(MediaId),
    /// A library asset, by id: it joins the project only when dragged out.
    Asset(String),
}

/// Media dropped on the timeline: what, where and onto which track.
type MediaDrop = (
    timeline_ui::TimelineDrag,
    FrameIdx,
    timeline_ui::MediaDropTarget,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Screen {
    /// The projects grid; no project is open.
    Home,
    #[default]
    Editor,
}

struct VenturiApp {
    screen: Screen,
    session: vv_session::Session,
    timeline_id: Option<TimelineId>,
    /// The timelines "above" `timeline_id`, from the root down, when one has
    /// entered a compound clip with a double click (see
    /// `enter_compound_timeline`): empty when editing the
    /// project timeline. The breadcrumb above the timeline shows them.
    timeline_stack: Vec<TimelineId>,
    /// Parallel to `timeline_stack`: the compound clip opened in that
    /// timeline, and its playhead then, to go back to where one was.
    entered_from: Vec<(Option<ClipId>, FrameIdx)>,
    /// Alt+Left or the mouse back button: handled as a click on the
    /// breadcrumb's parent segment.
    exit_compound_requested: bool,
    timeline_state: timeline_ui::TimelineState,
    media_pool_state: media_pool::MediaPoolState,
    keyframe_editor: keyframe_editor::KeyframeEditorState,
    mixer_panel: mixer_panel::MixerPanelState,
    plugin_editors: plugin_editors::PluginEditors,
    take: Option<voiceover::ActiveTake>,
    voiceover: voiceover::Panel,
    /// Media or elements not imported, shown in a separate window
    /// until the user closes it.
    import_warnings: Vec<String>,

    preview_meta: Option<vv_core::MediaMeta>,
    preview_error: Option<String>,
    /// Viewer texture registered in egui-wgpu once, then
    /// updated: registering a new one on every frame would lose the previous one.
    /// `None` without a shared device (tests).
    video_texture_id: Option<egui::TextureId>,
    /// Frame size in pixels at 100% zoom (the timeline resolution), not
    /// the texture's: that can be smaller, see `fit_output_size`.
    video_display_size: Option<egui::Vec2>,
    /// Layers and output of the texture's last composition: an identical
    /// one is skipped instead of re-uploading every plane on each repaint.
    viewer_content: Option<(Vec<frame_provider::OwnedLayer>, vv_render::OutputFrame)>,
    viewer_zoom: viewer_zoom::ViewerZoom,
    /// Player box and frame size of the last draw, for the zoom shortcuts.
    viewer_geometry: Option<(egui::Rect, egui::Vec2)>,
    /// Screen pixels per timeline pixel in the last draw of the viewer.
    viewer_scale: f64,
    /// What the viewer shows: the last composition, even if in this
    /// frame no new one was made (no empty flash).
    last_viewer_frame_kind: Option<ViewerFrameKind>,
    /// The frame in the viewer has a keyframe standing in for a media.
    viewer_keyframe: bool,
    /// egui-wgpu device/queue, shared with the compositor: a texture from
    /// another device cannot be registered in egui. `None` in the tests.
    egui_render_state: Option<eframe::egui_wgpu::RenderState>,
    /// The clip the Color window edits, which its scopes measure while the
    /// playhead is on it.
    scope_target: Option<PanelTarget>,
    /// That clip composed by itself (see `Isolation`): the stack, the frame
    /// and the texture, reused while the stack renders the same; and a
    /// counter bumped with each new composition.
    scope_frame: Option<(
        Vec<frame_provider::OwnedLayer>,
        vv_render::OutputFrame,
        vv_render::PooledTexture,
    )>,
    scope_generation: u64,
    /// Created the first time the Color window shows them.
    scopes: Option<vv_render::Scopes>,
    scope_view: color_window::ScopeView,
    /// Video buffer of the media pool preview: a `RenderAhead` on a
    /// timeline with only the clip of the media, and the id of the media in there.
    browsing_render_ahead: Option<(render_ahead::RenderAhead, MediaId)>,
    /// Alive only during a slip drag.
    slip_viewer: Option<slip_viewer::SlipViewer>,

    /// Created on the first import; thrown away when the user turns the proxies off.
    proxy_worker: Option<proxy_worker::ProxyWorker>,
    /// The export paused the proxies and must resume them; `false` if the
    /// pause was the user's.
    proxy_paused_for_export: bool,
    /// Generates in the background the waveforms of the media with audio. Created on the first
    /// import.
    waveform_worker: Option<waveform_worker::WaveformWorker>,
    thumbnail_worker: Option<thumbnail_worker::ThumbnailWorker>,
    /// Media pool thumbnails per `content_hash`; `None` = request in
    /// progress or failed (avoids requeuing it on every frame).
    thumbnails: HashMap<u64, Option<egui::TextureHandle>>,
    /// Waveforms read from the cache files, per `(content_hash, stream_index)`:
    /// the timeline draws them on every frame, re-reading them from disk it does not.
    waveform_cache: HashMap<(u64, usize), vv_media::Waveform>,
    /// Waveforms looked for on disk and not found: retried only when the
    /// worker reports them ready, not on every frame.
    waveform_missing: std::collections::HashSet<(u64, usize)>,
    /// Compound clips whose waveform was composed with sources
    /// not ready yet: to be redone when they arrive.
    waveform_partial: std::collections::HashSet<(u64, usize)>,

    /// Video clip shown in the viewer (the one under the playhead), from which
    /// frames and transform are read. `None` on a gap or during the preview
    /// from the media pool.
    active_clip: Option<(usize, ClipId)>,
    compositor: vv_render::Compositor,
    /// Opened on the first change made with the pointer down, closed on
    /// release: a drag is a single undo step.
    edit_drag_group: Option<vv_core::GroupMark>,
    /// The mask of that clip whose handles the viewer shows instead of the
    /// transform's.
    mask_focus: Option<(ClipId, usize)>,
    mask_overlay: mask_overlay::MaskOverlayState,

    /// Last handled playhead: it tells the one moved by the clock (no seek)
    /// from the one moved by the user.
    last_synced_playhead: FrameIdx,

    /// Media previewed from the pool or the library: the viewer shows it
    /// instead of the timeline, until the timeline is interacted with.
    browsing: Option<Browsed>,
    browse_playhead: FrameIdx,
    /// In/out of the preview: the portion dragged from the viewer onto the timeline.
    browse_marks: transport::MarkRange,
    browse_audio_streams: usize,

    /// Video buffer of the timeline. `None` until there is a timeline.
    render_ahead: Option<render_ahead::RenderAhead>,
    /// `history.generation()` at the last update of `render_ahead`.
    render_ahead_generation: u64,

    /// Mixer of the audio tracks and playback clock of the timeline.
    /// `None` until needed (in the tests it opens only if used).
    timeline_audio: Option<TimelineAudio>,

    /// Speed multiplier (1/2/4/8x) set by the "a" key; the
    /// space bar pauses and brings it back to 1x.
    playback_speed: f64,

    /// Moving the playhead, cutting or deleting selects the video clip
    /// under the playhead.
    selection_follows_playhead: bool,

    /// Audio during scrubbing (menu Timeline): on by default.
    scrub_audio: bool,
    resume_after_scrub: bool,

    arrow_hold: Option<ArrowHold>,
    /// J held: the playhead walks backwards.
    reverse_shuttle: Option<timeline_commands::ReverseShuttle>,
    /// The frame the preview axis last sent the player to.
    hover_jumped: Option<FrameIdx>,

    /// Snapping: in drags the clips snap to the nearby edges.
    snapping_enabled: bool,
    /// Transform handles over the viewer (button under the viewer).
    show_transform_overlay: bool,
    overlay_drag: Option<viewer_overlay::OverlayDrag>,
    details_editing: bool,

    /// X and Y zoom of the Transform panel kept together (the lock between the
    /// two fields): a UI preference, not a project one.
    zoom_link: bool,

    /// Tab open in the properties panel.
    properties_tab: PropertiesTab,
    clip_tab: properties_tabs::ClipTab,
    fonts: FontCatalog,

    export: Option<ExportUiState>,
    export_dialog: Option<export_dialog::ExportDialog>,
    new_timeline_dialog: Option<NewTimelineDialog>,
    paste_attributes: Option<PasteAttributesDialog>,
    library: library::Library,
    project_copier: Option<library_copy::ProjectCopier>,
    effects_panel: media_pool_ui::EffectsPanelState,
    library_ui: library_ui::LibraryUiState,
    library_ribbon: library_ribbon::RibbonState,
    stickers: stickers_ui::StickerState,
    fx: fx_library::FxState,
    toast: Option<(String, std::time::Instant)>,
    downloads: downloader_ui::DownloadState,
    /// Per file, see `first_media::probe_rotation`.
    media_rotation: HashMap<PathBuf, u16>,
    preset_dialog: Option<presets::PresetDialog>,
    /// A clipboard read for a paste in flight, see `request_clipboard_image`.
    clipboard_image_rx: Option<mpsc::Receiver<Option<paste_image::ClipboardImage>>>,
    /// A VST3 scan in flight, see `start_plugin_scan`.
    plugin_scan_rx: Option<mpsc::Receiver<Vec<vv_plugin::PluginInfo>>>,
    paste_image_dialog: Option<paste_image::PasteImageDialog>,
    paste_image_needs_save: bool,
    paste_key_watch: paste_image::PasteKeyWatch,
    /// Tells this instance's copies from other instances' ones.
    clipboard_instance: u64,
    /// Media copied from the pool, in this project's ids.
    copied_media: Vec<MediaId>,
    /// Paste from the menu: the clipboard's text arrives as a `Paste`
    /// event the next frame.
    menu_paste_pending: bool,
    speed_dialog: Option<speed_dialog::SpeedDialog>,
    silence_dialog: Option<silence_dialog::SilenceDialog>,
    /// Proposed again at the next silence removal of the session.
    silence_params: vv_session::silence::SilenceParams,
    /// Speech detected in the streams already analyzed, per
    /// `(content_hash, stream)`.
    silence_speech: HashMap<(u64, usize), vv_session::silence::SpeechAnalysis>,
    /// Attributes ticked in the last "paste attributes": proposed again the
    /// next time, as in the other NLEs.
    paste_attributes_selection: std::collections::HashSet<paste_attributes::Attribute>,
    paste_attributes_keyframe_mode: paste_attributes::KeyframeMode,
    /// Proposed again at the next export of the session.
    last_export_settings: Option<export::ExportSettings>,

    /// Last title sent to the compositor: the command is sent only when it changes.
    window_title: String,
    about_icon: Option<egui::TextureHandle>,
    /// Open, import or exit waiting for the answer to "save the
    /// changes?".
    pending_project_switch: Option<ProjectSwitch>,
    /// The user already answered for the exit: the next close request
    /// goes through.
    quit_confirmed: bool,
    /// Last project save/open error, shown in the
    /// toolbar next to the buttons — separate from `import_warnings` (those are
    /// for the media import, a different context).
    project_error: Option<String>,
    /// Outcome of the last relink from the media pool context menu,
    /// shown in a separate small window (see `show_relink_message`).
    relink_message: Option<String>,
    /// Base folder and media the last relink could not find by name.
    forced_relink_offer: Option<(PathBuf, Vec<MediaId>)>,
    forced_relink: Option<forced_relink_dialog::ForcedRelinkDialog>,
    /// Kept across dialogs within the session.
    forced_relink_criteria: forced_relink::Criteria,
    /// File dialog opened on a separate thread: on the GNOME/Wayland event
    /// loop thread it marks the app as unresponsive.
    pending_dialog: Option<PendingDialog>,

    /// Audio meter (toggle in View): a narrow band on the right
    /// of the timeline with the level of the outgoing audio. On by
    /// default, as in most NLEs.
    audiometer_enabled: bool,
    /// Meter levels, with a decay: the instantaneous peak would make
    /// the bars drop abruptly.
    audiometer_level: (f32, f32),
    viewer_fullscreen: bool,
    project_loading: Option<project_io::ProjectLoading>,
    matte_worker: matte_worker::MatteWorker,
    settings: settings::Settings,
    /// `None` in the tests: the settings are never written to disk.
    settings_path: Option<PathBuf>,
    settings_dialog: Option<settings_dialog::SettingsDialog>,
    about_open: bool,
    updater: updater_ui::State,
    project_settings_open: bool,
    /// Serving MCP to agents (see `mcp_host`); follows the setting.
    mcp: Option<mcp_host::McpHost>,
    /// `Session::epoch` last caught up with: the agent may replace the
    /// project while the UI is not drawing.
    seen_epoch: u64,
    /// `--mcp` on the command line: served whatever the setting says.
    mcp_forced: bool,
    #[cfg(target_os = "linux")]
    wayland_dnd: Option<wayland_dnd::WaylandDnd>,
    #[cfg(target_os = "linux")]
    wayland_gestures: Option<wayland_gestures::WaylandGestures>,
    #[cfg(target_os = "macos")]
    iso_key_down: bool,
    workspace: workspace::Workspace,
    autosave: autosave::Autosave,
    home: home::HomeState,
    /// The project name being edited in the top bar.
    project_name_edit: Option<String>,
    /// Width of the top bar's centre group in the last frame, to centre it.
    top_bar_center_width: f32,
    export_modal: Option<export_modal::ExportModal>,
    subtitles: subtitles::SubtitleState,
    auto_cut: auto_cut::State,
    auto_zoom: auto_zoom::State,
    reframe: reframe::State,
    dub: dub::State,
    beat: beat_sync::State,
    adjust: adjust_tab::AdjustState,
    stabilize: stabilize_ui::State,
    audio_sync: audio_sync::State,
    tracking: tracking_ui::State,
    vertical: vertical_layout::State,
    transcript: transcript::State,
    style_packs: style_packs::State,
    style_pack_previews: style_packs_ui::Previews,
    /// Blocks laid out in the main window this frame.
    block_rects: std::collections::HashSet<Block>,
    /// Read at startup and each time the overlay is switched on.
    safe_zone_platforms: (Vec<safe_zones::Platform>, Option<String>),
    ai_clips: ai_clips::State,
    ai_sequences: ai_sequence_ui::State,
    cover_picker: Option<cover::CoverPicker>,
    /// Name of the project reopened after a crash, until acknowledged.
    recovered_project: Option<String>,
}

impl Default for VenturiApp {
    fn default() -> Self {
        Self {
            screen: Screen::default(),
            session: vv_session::Session::default(),
            timeline_id: None,
            timeline_stack: Vec::new(),
            exit_compound_requested: false,
            entered_from: Vec::new(),
            timeline_state: timeline_ui::TimelineState::default(),
            media_pool_state: media_pool::MediaPoolState::default(),
            keyframe_editor: keyframe_editor::KeyframeEditorState::default(),
            mixer_panel: mixer_panel::MixerPanelState::default(),
            plugin_editors: Default::default(),
            take: None,
            voiceover: voiceover::Panel::default(),
            import_warnings: Vec::new(),
            preview_meta: None,
            preview_error: None,
            video_texture_id: None,
            video_display_size: None,
            viewer_content: None,
            viewer_zoom: viewer_zoom::ViewerZoom::default(),
            viewer_geometry: None,
            viewer_scale: 1.0,
            last_viewer_frame_kind: None,
            viewer_keyframe: false,
            egui_render_state: None,
            scope_target: None,
            scope_frame: None,
            scope_generation: 0,
            scopes: None,
            scope_view: color_window::ScopeView::default(),
            browsing_render_ahead: None,
            slip_viewer: None,
            proxy_worker: None,
            proxy_paused_for_export: false,
            waveform_worker: None,
            thumbnail_worker: None,
            thumbnails: HashMap::new(),
            waveform_cache: HashMap::new(),
            waveform_missing: Default::default(),
            waveform_partial: Default::default(),
            active_clip: None,
            compositor: vv_render::Compositor::new_headless(),
            edit_drag_group: None,
            mask_focus: None,
            mask_overlay: Default::default(),
            last_synced_playhead: 0,
            browsing: None,
            browse_playhead: 0,
            browse_marks: transport::MarkRange::default(),
            browse_audio_streams: 0,
            render_ahead: None,
            render_ahead_generation: 0,
            timeline_audio: None,
            playback_speed: 1.0,
            selection_follows_playhead: true,
            scrub_audio: true,
            resume_after_scrub: false,
            arrow_hold: None,
            reverse_shuttle: None,
            hover_jumped: None,
            snapping_enabled: true,
            show_transform_overlay: true,
            overlay_drag: None,
            details_editing: false,
            zoom_link: true,
            properties_tab: PropertiesTab::Video,
            clip_tab: properties_tabs::ClipTab::default(),
            fonts: FontCatalog::default(),
            export: None,
            export_dialog: None,
            new_timeline_dialog: None,
            paste_attributes: None,
            library: Default::default(),
            project_copier: None,
            effects_panel: Default::default(),
            library_ui: Default::default(),
            library_ribbon: Default::default(),
            stickers: Default::default(),
            fx: Default::default(),
            toast: None,
            downloads: Default::default(),
            media_rotation: Default::default(),
            preset_dialog: None,
            clipboard_image_rx: None,
            plugin_scan_rx: None,
            paste_image_dialog: None,
            paste_image_needs_save: false,
            paste_key_watch: Default::default(),
            clipboard_instance: {
                use std::hash::BuildHasher;
                std::collections::hash_map::RandomState::new().hash_one(std::process::id())
            },
            copied_media: Vec::new(),
            menu_paste_pending: false,
            speed_dialog: None,
            silence_dialog: None,
            silence_params: Default::default(),
            silence_speech: HashMap::new(),
            paste_attributes_selection: Default::default(),
            paste_attributes_keyframe_mode: paste_attributes::KeyframeMode::MaintainTiming,
            last_export_settings: None,
            window_title: String::new(),
            about_icon: None,
            pending_project_switch: None,
            quit_confirmed: false,
            project_error: None,
            relink_message: None,
            forced_relink_offer: None,
            forced_relink: None,
            forced_relink_criteria: Default::default(),
            pending_dialog: None,
            audiometer_enabled: true,
            audiometer_level: (0.0, 0.0),
            viewer_fullscreen: false,
            project_loading: None,
            matte_worker: matte_worker::MatteWorker::default(),
            settings: settings::Settings::default(),
            settings_path: None,
            settings_dialog: None,
            about_open: false,
            updater: Default::default(),
            project_settings_open: false,
            mcp: None,
            seen_epoch: 0,
            mcp_forced: false,
            #[cfg(target_os = "linux")]
            wayland_dnd: None,
            #[cfg(target_os = "linux")]
            wayland_gestures: None,
            #[cfg(target_os = "macos")]
            iso_key_down: false,
            workspace: workspace::Workspace::default(),
            autosave: autosave::Autosave::default(),
            home: home::HomeState::default(),
            project_name_edit: None,
            top_bar_center_width: 0.0,
            export_modal: None,
            subtitles: subtitles::SubtitleState::default(),
            auto_cut: auto_cut::State::default(),
            auto_zoom: auto_zoom::State::default(),
            reframe: reframe::State::default(),
            dub: dub::State::default(),
            beat: beat_sync::State::default(),
            adjust: Default::default(),
            stabilize: Default::default(),
            audio_sync: Default::default(),
            tracking: Default::default(),
            vertical: vertical_layout::State::default(),
            transcript: transcript::State::default(),
            style_packs: style_packs::State::default(),
            style_pack_previews: Default::default(),
            block_rects: Default::default(),
            safe_zone_platforms: safe_zones::load(),
            ai_clips: ai_clips::State::default(),
            ai_sequences: ai_sequence_ui::State::default(),
            cover_picker: None,
            recovered_project: None,
        }
    }
}

impl VenturiApp {
    /// `targets` plus, when one is a subtitle of a linked timeline, every
    /// other subtitle: the look is edited on all of them together.
    fn with_linked_subtitles(&self, targets: &[PanelTarget]) -> Vec<PanelTarget> {
        let mut all = targets.to_vec();
        let Some(timeline_id) = self.timeline_id else {
            return all;
        };
        let timeline = &self.session.project.timelines[timeline_id];
        let on_subtitles = |t: &PanelTarget| {
            t.is_text
                && timeline
                    .tracks
                    .get(t.track_index)
                    .is_some_and(|tr| tr.subtitles)
        };
        if !targets.iter().any(on_subtitles)
            || !vv_core::captions::subtitles_linked(&self.session.project, timeline_id)
        {
            return all;
        }
        let Some(index) = vv_core::captions::subtitle_track(timeline) else {
            return all;
        };
        for clip in &timeline.tracks[index].clips {
            let is_text = matches!(clip.source, vv_core::ClipSource::Text);
            if is_text && !all.iter().any(|t| t.clip_id == clip.id) {
                all.push(panel_target(
                    timeline_id,
                    index,
                    clip,
                    self.timeline_state.playhead,
                ));
            }
        }
        all
    }

    /// Stereo output peak meter, with decay.
    fn draw_audiometer(&mut self, ui: &mut egui::Ui) {
        const DECAY: f32 = 0.85;
        let (raw_l, raw_r) = self
            .timeline_audio
            .as_ref()
            .map(TimelineAudio::peak_linear_stereo)
            .unwrap_or((0.0, 0.0));
        let (level_l, level_r) = &mut self.audiometer_level;
        *level_l = raw_l.max(*level_l * DECAY);
        *level_r = raw_r.max(*level_r * DECAY);
        let (level_l, level_r) = (level_l.clamp(0.0, 1.0), level_r.clamp(0.0, 1.0));

        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_gray(20));

        let margin = 4.0;
        let gap = 2.0;
        let full_bar_rect = rect.shrink(margin);
        let bar_width = (full_bar_rect.width() - gap) / 2.0;
        let left_rect = egui::Rect::from_min_size(
            full_bar_rect.left_top(),
            egui::vec2(bar_width, full_bar_rect.height()),
        );
        let right_rect = egui::Rect::from_min_size(
            full_bar_rect.left_top() + egui::vec2(bar_width + gap, 0.0),
            egui::vec2(bar_width, full_bar_rect.height()),
        );

        for (bar_rect, level) in [(left_rect, level_l), (right_rect, level_r)] {
            painter.rect_filled(bar_rect, 2.0, egui::Color32::from_gray(10));
            if level > 0.0 {
                let fill_height = bar_rect.height() * level;
                let fill_rect = egui::Rect::from_min_max(
                    egui::pos2(bar_rect.left(), bar_rect.bottom() - fill_height),
                    bar_rect.right_bottom(),
                );
                // Green up to 70%, yellow up to 90%, red past that
                // (near clipping) — the same convention as a common
                // VU meter.
                let color = if level > 0.9 {
                    crate::theme::ERROR
                } else if level > 0.7 {
                    crate::theme::WARNING
                } else {
                    crate::theme::SUCCESS
                };
                painter.rect_filled(fill_rect, 2.0, color);
            }
        }

        // Repaint while the bars are falling.
        if level_l > 0.001 || level_r > 0.001 {
            ui.ctx().request_repaint();
        }
    }

    fn preview_media(&mut self, media_id: MediaId) {
        self.preview(&Browsed::Media(media_id));
    }

    /// The pool item or the library asset's media.
    fn browsed_item(&self, browsed: &Browsed) -> Option<&vv_core::MediaItem> {
        match browsed {
            Browsed::Media(id) => self.session.project.media_pool.get(*id),
            Browsed::Asset(id) => self.library.assets.iter().find(|a| a.id == *id)?.item(),
        }
    }

    fn browsing_item(&self) -> Option<&vv_core::MediaItem> {
        self.browsed_item(self.browsing.as_ref()?)
    }

    /// "Raw" preview of a media, not tied to the timeline: it shows the
    /// first frame from a dedicated decode-ahead.
    fn preview(&mut self, browsed: &Browsed) {
        self.browsing_render_ahead = None;
        let Some(item) = self.browsed_item(browsed).cloned() else {
            return;
        };
        self.stop_voiceover();
        let (path, meta) = (item.path.clone(), item.meta.clone());
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
        }
        self.reset_playback_speed_to_normal();
        self.browse_audio_streams = meta.audio_stream_count();
        let has_video = meta.has_video;
        self.preview_meta = Some(meta);
        self.preview_error = None;
        self.browse_playhead = 0;
        self.browse_marks = transport::MarkRange::default();
        if !has_video {
            // A library asset may have none yet; one already on disk is skipped.
            if item.meta.has_audio && item.compound.is_none() {
                let secs = item.meta.duration_frames as f64 / item.meta.fps.as_f64().max(1e-9);
                self.waveform_worker
                    .get_or_insert_with(waveform_worker::WaveformWorker::spawn)
                    .enqueue(
                        path,
                        item.content_hash,
                        item.meta.audio_stream_count(),
                        vv_media::recommended_num_peaks(secs),
                    );
            }
            return;
        }
        // The buffer opens the file on its own and skips the ones that do not
        // open: the error must be seen here.
        if let Err(e) = vv_media::Decoder::open(&path) {
            self.preview_error = Some(e.to_string());
            return;
        }
        self.browsing_render_ahead = Some(self.spawn_browsing_render_ahead(item));
    }

    /// A timeline at the media fps with only it on top: the timeline frames
    /// coincide with the source ones.
    fn spawn_browsing_render_ahead(
        &self,
        item: vv_core::MediaItem,
    ) -> (render_ahead::RenderAhead, MediaId) {
        self.spawn_media_render_ahead(
            item,
            self.settings.cache_budget_bytes,
            self.settings.lookahead_secs,
            self.settings.behind_secs,
        )
    }

    fn spawn_media_render_ahead(
        &self,
        item: vv_core::MediaItem,
        cache_budget_bytes: usize,
        lookahead_secs: f64,
        behind_secs: f64,
    ) -> (render_ahead::RenderAhead, MediaId) {
        let meta = item.meta.clone();
        let mut project = vv_core::Project::default();
        let preview_media = project.media_pool.insert(item);
        let mut track = Track::new(TrackKind::Video);
        track.clips.push(vv_core::Clip::from_source_range(
            project.alloc_clip_id(),
            vv_core::ClipSource::Media(preview_media),
            0,
            meta.duration_frames.max(1),
            0,
            vv_core::Rational::one(),
        ));
        let timeline_id = project.timelines.insert(vv_core::Timeline {
            name: "preview".into(),
            fps: meta.fps,
            resolution: (meta.width, meta.height),
            tracks: vec![track],
            markers: Vec::new(),
            master: Default::default(),
        });
        let render_ahead = render_ahead::RenderAhead::spawn(
            project,
            timeline_id,
            cache_budget_bytes,
            self.settings.proxy(),
            lookahead_secs,
            behind_secs,
        );
        render_ahead.set_preview_divisor(self.settings.preview_resolution.divisor());
        (render_ahead, preview_media)
    }

    pub(crate) fn apply_preview_resolution(&self) {
        let divisor = self.settings.preview_resolution.divisor();
        for render_ahead in self.render_aheads() {
            render_ahead.set_preview_divisor(divisor);
        }
    }

    /// The timeline buffer and the preview one, if they exist.
    fn render_aheads(&self) -> impl Iterator<Item = &render_ahead::RenderAhead> {
        self.render_ahead
            .iter()
            .chain(self.browsing_render_ahead.as_ref().map(|(r, _)| r))
    }

    /// The active Video clip (topmost track among those having one
    /// at that point, `Timeline::active_video_clip_at`) at frame `frame`,
    /// with its track — the one the viewer shows.
    fn active_video_clip_at(&self, frame: FrameIdx) -> Option<(usize, ClipId)> {
        let timeline_id = self.timeline_id?;
        self.session.project.timelines[timeline_id]
            .active_video_clip_at(frame)
            .map(|(t, c)| (t, c.id))
    }

    /// With "selection follows playhead" on, it selects the video clip under
    /// the playhead and its group (nothing on a gap).
    fn sync_selection_to_playhead(&mut self) {
        // During playback the selection is the user's: losing a selected
        // audio clip because the video under the playhead changed is a trap.
        if !self.selection_follows_playhead || self.is_timeline_playing() {
            return;
        }
        let Some((track_index, clip_id)) = self.active_video_clip_at(self.timeline_state.playhead)
        else {
            self.timeline_state.set_single_selection(None);
            return;
        };
        let mut selected = BTreeSet::from([(track_index, clip_id)]);
        if let Some(timeline_id) = self.timeline_id {
            selected.extend(
                self.session.project.timelines[timeline_id].linked_members(track_index, clip_id),
            );
        }
        self.timeline_state
            .set_selection(selected, Some((track_index, clip_id)));
        if let Some(timeline_id) = self.timeline_id {
            self.timeline_state
                .drop_locked(&self.session.project.timelines[timeline_id]);
        }
    }

    /// Aligns the viewer clip to the playhead and, if the user moved it
    /// (`force_seek` even during playback), the audio clock.
    fn ensure_active_clip_matches_playhead(&mut self, force_seek: bool) {
        if self.browsing.is_some() {
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let playhead = self.timeline_state.playhead;
        self.active_clip = self.active_video_clip_at(playhead);
        let user_moved_playhead =
            playhead != self.last_synced_playhead && (force_seek || !self.is_timeline_playing());
        if user_moved_playhead {
            if let Some(audio) = &mut self.timeline_audio {
                audio.seek_frame(
                    playhead,
                    self.session.project.timelines[timeline_id].fps.as_f64(),
                );
            }
            if let Some(render_ahead) = &self.render_ahead {
                render_ahead.jump_to(playhead);
            }
        }
        self.last_synced_playhead = playhead;
    }

    /// Pauses while the playhead is held and resumes on release.
    fn hold_playback_while_scrubbing(&mut self, held: bool) {
        if held {
            if self.is_timeline_playing() {
                self.toggle_playback();
                self.resume_after_scrub = true;
            }
        } else if std::mem::take(&mut self.resume_after_scrub) && !self.is_timeline_playing() {
            self.toggle_playback();
        }
    }

    fn is_timeline_playing(&self) -> bool {
        self.timeline_audio
            .as_ref()
            .is_some_and(TimelineAudio::is_playing)
    }

    fn timeline_audio(&mut self) -> &mut TimelineAudio {
        self.timeline_audio.get_or_insert_with(TimelineAudio::new)
    }

    fn sync_timeline_audio(&mut self) {
        let fps = self.browse_fps();
        let browsed = self
            .browsing_item()
            .map(|item| (item.path.clone(), item.meta.clone()));
        let Some(audio) = &mut self.timeline_audio else {
            return;
        };
        audio.set_budget_bytes(self.settings.audio_budget_bytes);
        if self.browsing.is_some() {
            if let Some((path, meta)) = browsed {
                let duration_secs = meta.duration_frames as f64 / meta.fps.as_f64().max(1e-9);
                audio.sync_media(&path, self.browse_audio_streams, fps, duration_secs);
            }
        } else if let Some(timeline_id) = self.timeline_id {
            audio.sync(
                &self.session.project,
                timeline_id,
                self.session.history.generation(),
            );
        } else {
            audio.clear_media();
        }
    }

    fn stop_browsing(&mut self) {
        if self.browsing.is_some() && self.is_timeline_playing() {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
        }
        self.browsing = None;
        self.browsing_render_ahead = None;
        self.preview_meta = None;
        // The clock stayed at the preview position: force the seek to the
        // timeline playhead.
        self.last_synced_playhead = FrameIdx::MIN;
    }

    /// The viewer shows the waveform of the browsed media.
    fn browsing_audio_only(&self) -> bool {
        self.browsing.is_some() && self.preview_meta.as_ref().is_some_and(|m| !m.has_video)
    }

    fn browse_total_frames(&self) -> FrameIdx {
        self.preview_meta.as_ref().map_or(0, |m| m.duration_frames)
    }

    fn browse_fps(&self) -> f64 {
        self.preview_meta
            .as_ref()
            .map_or(1.0, |m| m.fps.as_f64().max(1e-9))
    }

    /// The mixer clock acts as the playhead for the preview too, as for the
    /// timeline.
    fn toggle_browse_playback(&mut self) {
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            return;
        }
        let total = self.browse_total_frames();
        if total <= 0 {
            return;
        }
        if self.browse_playhead >= total - 1 {
            self.browse_playhead = 0;
        }
        self.seek_browse(self.browse_playhead);
        self.timeline_audio();
        self.sync_timeline_audio();
        self.timeline_audio().play();
    }

    fn seek_browse(&mut self, frame: FrameIdx) {
        let frame = frame.clamp(0, (self.browse_total_frames() - 1).max(0));
        self.browse_playhead = frame;
        let fps = self.browse_fps();
        if let Some(audio) = &mut self.timeline_audio {
            audio.seek_frame(frame, fps);
        }
        if let Some((render_ahead, _)) = &self.browsing_render_ahead {
            render_ahead.jump_to(frame);
        }
    }

    fn drive_browse_playback(&mut self) {
        if self.browsing.is_none() || !self.is_timeline_playing() {
            return;
        }
        let fps = self.browse_fps();
        let last = (self.browse_total_frames() - 1).max(0);
        let frame = self.timeline_audio().position_frame(fps);
        self.browse_playhead = frame.min(last);
        if frame >= last {
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            self.timeline_audio().seek_frame(last, fps);
        }
    }

    fn mark_at_playhead(&mut self, is_in: bool) {
        let Some(target) = self.mark_target() else {
            return;
        };
        if target.total <= 0 {
            return;
        }
        if is_in {
            target.marks.set_in(target.playhead, target.total);
        } else {
            target.marks.set_out(target.playhead, target.total);
        }
    }

    fn mark_target(&mut self) -> Option<MarkTarget<'_>> {
        if self.browsing.is_some() {
            if self.preview_error.is_some() {
                None
            } else {
                let total = self.browse_total_frames();
                let playhead = self.browse_playhead;
                Some(MarkTarget {
                    marks: &mut self.browse_marks,
                    total,
                    playhead,
                })
            }
        } else if let Some(timeline_id) = self.timeline_id {
            let total = self.session.project.timelines[timeline_id].total_frames();
            let playhead = self.timeline_state.playhead;
            Some(MarkTarget {
                marks: &mut self.timeline_state.export_marks,
                total,
                playhead,
            })
        } else {
            None
        }
    }

    fn clear_in_point(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_in();
        }
    }

    fn clear_out_point(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_out();
        }
    }

    fn clear_in_and_out(&mut self) {
        if let Some(target) = self.mark_target() {
            target.marks.clear_all();
        }
    }

    /// Left/right arrows: one frame back/forward, holding them down
    /// scrolls at `ARROW_HOLD_SPEED`. It pauses if playing.
    /// Returns `true` while an arrow is held.
    fn step_playhead_with_arrows(&mut self, direction: Option<FrameIdx>, time: f64) -> bool {
        let Some(direction) = direction else {
            self.arrow_hold = None;
            return false;
        };
        let Some(timeline_id) = self.timeline_id else {
            return false;
        };
        if self
            .arrow_hold
            .as_ref()
            .is_none_or(|h| h.direction != direction)
        {
            if self.is_timeline_playing() {
                self.toggle_playback();
            }
            self.arrow_hold = Some(ArrowHold {
                direction,
                pressed_at: time,
                start_frame: self.timeline_state.playhead,
            });
        }
        let hold = self.arrow_hold.as_ref().expect("set above");
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let target = arrow_hold_target(hold, time - hold.pressed_at, fps);
        if target != self.timeline_state.playhead {
            self.timeline_state.playhead = target;
            self.ensure_active_clip_matches_playhead(true);
            self.sync_selection_to_playhead();
            self.play_scrub_audio();
        }
        true
    }

    fn play_scrub_audio(&mut self) {
        if !self.scrub_audio || self.browsing.is_some() {
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let fps = self.session.project.timelines[timeline_id].fps.as_f64();
        let playhead = self.timeline_state.playhead;
        self.timeline_audio();
        self.sync_timeline_audio();
        let audio = self.timeline_audio();
        audio.seek_frame(playhead, fps);
        audio.play_scrub_snippet();
    }

    /// Play/pause of the timeline from the playhead, with no need for a selection:
    /// gaps included (they play silence), stopping only past the end of the
    /// content.
    pub(crate) fn toggle_playback(&mut self) {
        if self.browsing.is_some() {
            self.toggle_browse_playback();
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if self.is_timeline_playing() {
            self.timeline_audio().pause();
            // The space bar always pauses at 1x: a later "a"
            // restarts at normal speed.
            self.reset_playback_speed_to_normal();
            self.finish_take();
            return;
        }
        let timeline = &self.session.project.timelines[timeline_id];
        let (fps, end) = (timeline.fps.as_f64(), timeline.total_frames());
        let playhead = self.timeline_state.playhead;
        if playhead >= end || !self.prepare_take() {
            return;
        }
        self.cue_timeline_playback(playhead, fps);
        self.start_take(playhead);
        self.timeline_audio().play();
    }

    fn cue_timeline_playback(&mut self, playhead: FrameIdx, fps: f64) {
        self.timeline_audio();
        self.sync_timeline_audio();
        self.timeline_audio().seek_frame(playhead, fps);
        self.last_synced_playhead = playhead;
        self.active_clip = self.active_video_clip_at(playhead);
    }

    /// During playback the playhead follows the mixer clock; at the end of the
    /// content it stops.
    fn drive_playback(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if !self.is_timeline_playing() {
            return;
        }
        let timeline = &self.session.project.timelines[timeline_id];
        let (fps, end) = (timeline.fps.as_f64(), timeline.total_frames());
        let mut frame = self.timeline_audio().position_frame(fps);
        if frame >= end {
            frame = end;
            self.timeline_audio().pause();
            self.reset_playback_speed_to_normal();
            self.timeline_audio().seek_frame(end, fps);
        }
        self.timeline_state.playhead = frame;
        self.active_clip = self.active_video_clip_at(frame);
    }

    /// "a" key: when stopped like the space bar (1x), during playback it
    /// accelerates 2x -> 4x -> 8x. Only the space bar pauses.
    fn handle_fast_playback_key(&mut self) {
        if self.browsing.is_some() {
            if !self.is_timeline_playing() {
                self.toggle_browse_playback();
            }
            return;
        }
        if !self.is_timeline_playing() {
            self.toggle_playback();
            return;
        }
        let next_speed = match SpeedTier::from_multiplier(self.playback_speed) {
            Some(tier) => tier.next().tempo(),
            None => SpeedTier::X2.tempo(),
        };
        self.request_playback_speed(next_speed);
    }

    fn reset_playback_speed_to_normal(&mut self) {
        self.request_playback_speed(1.0);
    }

    /// 1x immediately; the fast speeds when their stretched audio
    /// is ready (see `TimelineAudio::request_speed`).
    fn request_playback_speed(&mut self, speed: f64) {
        match &mut self.timeline_audio {
            Some(audio) => {
                audio.request_speed(speed);
                self.playback_speed = audio.speed();
            }
            None => self.playback_speed = speed,
        }
    }

    /// Timeline intervals already decoded, for the "buffered" strip.
    fn buffered_timeline_ranges(&self) -> Vec<(FrameIdx, FrameIdx)> {
        let Some(timeline_id) = self.timeline_id else {
            return Vec::new();
        };
        if self.render_ahead.is_none() {
            return Vec::new();
        }
        let mut cached: HashMap<MediaId, Vec<(FrameIdx, FrameIdx)>> = HashMap::new();
        self.cached_ranges_of_timeline(timeline_id, &mut cached, 0, false)
    }

    /// Timeline intervals keyframes stand in for, for the strip in orange.
    fn keyframe_timeline_ranges(&self) -> Vec<(FrameIdx, FrameIdx)> {
        let Some(timeline_id) = self.timeline_id else {
            return Vec::new();
        };
        let mut cached: HashMap<MediaId, Vec<(FrameIdx, FrameIdx)>> = HashMap::new();
        self.cached_ranges_of_timeline(timeline_id, &mut cached, 0, true)
    }

    /// Decoded intervals of `timeline_id`, in frames of that
    /// timeline. A compound clip has no cached frames of its own (it is not a
    /// media to decode): its intervals are those of its nested
    /// timeline, remapped through the clip. With `keyframes`, the intervals
    /// keyframes stand in for instead.
    fn cached_ranges_of_timeline(
        &self,
        timeline_id: TimelineId,
        cached: &mut HashMap<MediaId, Vec<(FrameIdx, FrameIdx)>>,
        depth: u32,
        keyframes: bool,
    ) -> Vec<(FrameIdx, FrameIdx)> {
        let (Some(render_ahead), Some(timeline)) = (
            &self.render_ahead,
            self.session.project.timelines.get(timeline_id),
        ) else {
            return Vec::new();
        };
        let clips: Vec<(MediaId, &vv_core::Clip)> = timeline
            .visible_video_clips()
            .filter_map(|(_, clip)| match clip.source {
                vv_core::ClipSource::Media(media_id) => Some((media_id, clip)),
                _ => None,
            })
            .collect();
        let mut ranges = Vec::new();
        for (media_id, clip) in clips {
            if !cached.contains_key(&media_id) {
                let nested = self
                    .session
                    .project
                    .media_pool
                    .get(media_id)
                    .and_then(|item| item.compound)
                    .filter(|_| depth < vv_core::MAX_COMPOUND_DEPTH);
                let source_ranges = match nested {
                    Some(nested_id) => {
                        self.cached_ranges_of_timeline(nested_id, cached, depth + 1, keyframes)
                    }
                    None if keyframes => render_ahead.keyframe_ranges_for(media_id),
                    None => render_ahead.cached_ranges_for(media_id),
                };
                cached.insert(media_id, source_ranges);
            }
            ranges.extend(map_source_ranges_to_timeline(clip, &cached[&media_id]));
        }
        ranges
    }

    /// Timeline intervals of the Media clips served by the proxy (the whole
    /// clip, not only the part already decoded).
    fn proxy_timeline_ranges(&self) -> Vec<(FrameIdx, FrameIdx)> {
        let Some(timeline_id) = self.timeline_id else {
            return Vec::new();
        };
        if self.settings.proxy().is_none() {
            return Vec::new();
        }
        let Some(proxy_worker) = &self.proxy_worker else {
            return Vec::new();
        };
        self.session.project.timelines[timeline_id]
            .visible_video_clips()
            .filter(|(_, clip)| {
                let vv_core::ClipSource::Media(media_id) = &clip.source else {
                    return false;
                };
                self.session
                    .project
                    .media_pool
                    .get(*media_id)
                    .is_some_and(|item| {
                        proxy_worker.state(item.content_hash)
                            == Some(proxy_worker::ProxyState::Ready)
                    })
            })
            .map(|(_, clip)| (clip.timeline_start, clip.timeline_end() - 1))
            .collect()
    }

    /// Loads into `waveform_cache` the waveforms of the audio clips on the timeline,
    /// reading the cache file once per key.
    fn ensure_waveforms_loaded(&mut self) {
        if let Some(worker) = &self.waveform_worker {
            let arrived = worker.drain_ready();
            for key in &arrived {
                self.waveform_missing.remove(key);
            }
            // The compounds composed with still missing sources must be
            // redone now that some of them have arrived.
            if !arrived.is_empty() {
                for key in self.waveform_partial.drain() {
                    self.waveform_cache.remove(&key);
                }
            }
        }
        for key in self.browsed_waveform_keys() {
            self.load_waveform(key);
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let mut compounds: Vec<vv_core::MediaId> = Vec::new();
        let mut files: Vec<(u64, usize)> = Vec::new();
        for (_, track) in
            self.session.project.timelines[timeline_id].tracks_of_kind(TrackKind::Audio)
        {
            for clip in &track.clips {
                let vv_core::ClipSource::Media(media_id) = &clip.source else {
                    continue;
                };
                let Some(item) = self.session.project.media_pool.get(*media_id) else {
                    continue;
                };
                let key = (item.content_hash, clip.audio_stream_index);
                if !item.meta.has_audio
                    || self.waveform_cache.contains_key(&key)
                    || self.waveform_missing.contains(&key)
                {
                    continue;
                }
                if item.compound.is_some() {
                    compounds.push(*media_id);
                    continue;
                }
                files.push(key);
            }
        }
        for key in files {
            self.load_waveform(key);
        }
        for media_id in compounds {
            self.ensure_compound_waveform(media_id, 0);
        }
    }

    /// One waveform per audio stream of the media browsed in the viewer.
    fn browsed_waveform_keys(&self) -> Vec<(u64, usize)> {
        self.browsing_item()
            .filter(|item| item.meta.has_audio && item.compound.is_none())
            .map(|item| {
                (0..item.meta.audio_stream_count())
                    .map(|stream| (item.content_hash, stream))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Reads waveform `key` from its cache file, or notes it missing.
    fn load_waveform(&mut self, key: (u64, usize)) {
        if self.waveform_cache.contains_key(&key) || self.waveform_missing.contains(&key) {
            return;
        }
        match vv_media::waveform::load_waveform(key.0, key.1) {
            Some(waveform) => {
                self.waveform_cache.insert(key, waveform);
            }
            None => {
                self.waveform_missing.insert(key);
            }
        }
    }

    /// Waveform of a compound clip: there is no file to decode, it is
    /// composed from those of the clips of its nested timeline (see
    /// `compose_compound_waveform`), loading them first if needed.
    fn ensure_compound_waveform(&mut self, media_id: vv_core::MediaId, depth: u32) {
        if depth >= vv_core::MAX_COMPOUND_DEPTH {
            return;
        }
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return;
        };
        let (Some(nested_id), key) = (item.compound, (item.content_hash, 0)) else {
            return;
        };
        if self.waveform_cache.contains_key(&key) {
            return;
        }
        let sources: Vec<(vv_core::MediaId, usize)> = self.session.project.timelines[nested_id]
            .tracks_of_kind(TrackKind::Audio)
            .flat_map(|(_, track)| track.clips.iter())
            .filter_map(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => Some((id, clip.audio_stream_index)),
                _ => None,
            })
            .collect();
        for (source_id, stream) in sources {
            let Some(source) = self.session.project.media_pool.get(source_id) else {
                continue;
            };
            if source.compound.is_some() {
                self.ensure_compound_waveform(source_id, depth + 1);
                continue;
            }
            let source_key = (source.content_hash, stream);
            if self.waveform_cache.contains_key(&source_key)
                || self.waveform_missing.contains(&source_key)
            {
                continue;
            }
            match vv_media::waveform::load_waveform(source_key.0, source_key.1) {
                Some(waveform) => {
                    self.waveform_cache.insert(source_key, waveform);
                }
                None => {
                    self.waveform_missing.insert(source_key);
                }
            }
        }
        let Some((waveform, complete)) =
            compose_compound_waveform(&self.session.project, &self.waveform_cache, media_id)
        else {
            return;
        };
        if !complete {
            self.waveform_partial.insert(key);
        }
        self.waveform_cache.insert(key, waveform);
    }

    fn ensure_timeline(&mut self) -> TimelineId {
        self.ensure_timeline_with(vv_core::Rational::new(25, 1), (1920, 1080), true, None)
    }

    /// Like `ensure_timeline`, but a new timeline takes its format from the
    /// first media (see `first_media`; media with video only).
    fn ensure_timeline_for(
        &mut self,
        meta: &vv_core::MediaMeta,
        rotation: u16,
        import: Option<vv_session::JobId>,
    ) -> TimelineId {
        let format = first_media::project_format(meta, rotation);
        if let Some(id) = self.timeline_id
            && !self.has_video_media(id)
        {
            self.set_project_fps(format.fps);
            self.set_project_resolution(format.resolution);
        }
        self.ensure_timeline_with(format.fps, format.resolution, true, import)
    }

    /// Whether the format still waits for a first video: titles and audio
    /// alone do not set it.
    fn has_video_media(&self, timeline_id: TimelineId) -> bool {
        let project = &self.session.project;
        project.timelines[timeline_id]
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => {
                    project.media_pool.get(id).is_some_and(|m| m.meta.has_video)
                }
                _ => false,
            })
    }

    /// Clockwise turn recorded in the media's file, probed once per path.
    fn media_rotation(&mut self, media_id: MediaId) -> u16 {
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return 0;
        };
        if item.compound.is_some() || !item.meta.has_video || item.meta.is_image() {
            return 0;
        }
        let path = item.path.clone();
        *self
            .media_rotation
            .entry(path.clone())
            .or_insert_with(|| first_media::probe_rotation(&path))
    }

    /// Turns a clip upright when its file carries a rotation the engine
    /// does not apply, through the clip's own transform.
    fn upright_clip(
        &mut self,
        timeline_id: TimelineId,
        clip: (usize, ClipId),
        media_id: MediaId,
        meta: &vv_core::MediaMeta,
    ) {
        let rotation = self.media_rotation(media_id);
        if rotation == 0 {
            return;
        }
        let canvas = self.session.project.timelines[timeline_id].resolution;
        let (turn, zoom) = first_media::rotation_fit((meta.width, meta.height), canvas, rotation);
        for (param, value) in [
            (vv_core::TransformParam::Rotation, turn),
            (vv_core::TransformParam::ZoomX, zoom),
            (vv_core::TransformParam::ZoomY, zoom),
        ] {
            self.session
                .apply(Box::new(vv_core::set_clip_transform_param(
                    timeline_id,
                    clip.0,
                    clip.1,
                    param,
                    value,
                )));
        }
    }

    /// Like `ensure_timeline`, but without a video track: a drop of audio only must
    /// not create an empty one.
    fn ensure_timeline_audio_only(&mut self, import: Option<vv_session::JobId>) -> TimelineId {
        self.ensure_timeline_with(vv_core::Rational::new(25, 1), (1920, 1080), false, import)
    }

    /// A new timeline is part of the undo step of `import`, if given, else
    /// a step of its own (or of the caller's group).
    fn ensure_timeline_with(
        &mut self,
        fps: vv_core::Rational,
        resolution: (u32, u32),
        include_video_track: bool,
        import: Option<vv_session::JobId>,
    ) -> TimelineId {
        if let Some(id) = self.timeline_id {
            return id;
        }
        let mut tracks = Vec::new();
        if include_video_track {
            tracks.push(Track::new_main());
        }
        tracks.push(Track::new(TrackKind::Audio));
        let timeline = vv_core::Timeline {
            name: "Timeline 1".into(),
            fps,
            resolution,
            tracks,
            markers: Vec::new(),
            master: Default::default(),
        };
        let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::NewTimeline);
        let (id, _) = add.timeline(&mut self.session.project, timeline, None);
        match import {
            Some(job) => self.session.join_import(job, Box::new(add)),
            None => self.session.apply(Box::new(add)),
        }
        self.timeline_id = Some(id);
        self.spawn_render_ahead_if_needed(id);
        id
    }

    /// New timeline of the project: like the initial one it also shows up in
    /// the media pool as a compound clip (see `ensure_timeline_with`).
    pub(crate) fn create_timeline(
        &mut self,
        name: String,
        fps: vv_core::Rational,
        resolution: (u32, u32),
    ) -> TimelineId {
        self.session.create_timeline(vv_core::Timeline {
            name,
            fps,
            resolution,
            tracks: vec![Track::new_main(), Track::new(TrackKind::Audio)],
            markers: Vec::new(),
            master: Default::default(),
        })
    }

    /// Opens a timeline as the top level one: the breadcrumb of the compound
    /// clips one was inside no longer applies.
    pub(crate) fn open_timeline(&mut self, timeline_id: TimelineId) {
        if self.timeline_id == Some(timeline_id) {
            return;
        }
        self.store_view();
        self.timeline_stack.clear();
        self.entered_from.clear();
        self.switch_to_timeline(timeline_id);
        self.restore_playhead();
    }

    /// Records the active timeline and its playhead in `project.view`; from
    /// inside compound clips, the root of the breadcrumb.
    pub(crate) fn store_view(&mut self) {
        let Some(current) = self.timeline_id else {
            return;
        };
        let (timeline, playhead) = match self.timeline_stack.first() {
            Some(&root) => (root, self.playhead_at_stack_level(0)),
            None => (current, self.timeline_state.playhead),
        };
        let project = &mut self.session.project;
        project.view.active_timeline = Some(timeline);
        project.view.playheads.insert(timeline, playhead);
        project
            .view
            .playheads
            .retain(|id, _| project.timelines.contains_key(*id));
    }

    /// Puts the playhead of the active timeline back where `project.view`
    /// left it.
    pub(crate) fn restore_playhead(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let Some(&frame) = self.session.project.view.playheads.get(&timeline_id) else {
            return;
        };
        let end = self.session.project.timelines[timeline_id].total_frames();
        self.timeline_state.playhead = frame.clamp(0, (end - 1).max(0));
        self.timeline_state.reveal_playhead = true;
    }

    /// Spawns `render_ahead` the first time a timeline exists (one
    /// per session: it is not recreated afterwards, only updated via
    /// `sync_render_ahead`/`RenderAhead::update_project`).
    fn spawn_render_ahead_if_needed(&mut self, timeline_id: TimelineId) {
        if self.render_ahead.is_none() {
            self.render_ahead = Some(render_ahead::RenderAhead::spawn(
                self.session.project.clone(),
                timeline_id,
                self.settings.cache_budget_bytes,
                self.settings.proxy(),
                self.settings.lookahead_secs,
                self.settings.behind_secs,
            ));
            self.apply_preview_resolution();
            self.render_ahead_generation = self.session.history.generation();
        }
    }

    /// Opens `nested_id` as if it were the project timeline, with the playhead
    /// on its frame `at`: from the double click on compound clip `clip`
    /// (`timeline_ui::show_timeline`). The whole
    /// existing editing UI stays unchanged (it is already parameterized on
    /// `self.timeline_id`), here only the "which" has to be moved and
    /// where one came from stacked for the breadcrumb.
    pub(crate) fn enter_compound_timeline(
        &mut self,
        nested_id: TimelineId,
        at: FrameIdx,
        clip: Option<ClipId>,
    ) {
        let Some(current) = self.timeline_id else {
            return;
        };
        if current == nested_id || self.timeline_stack.contains(&nested_id) {
            // Already on this level, or already an ancestor on the stack: a
            // residual cycle (see `vv_core::MAX_COMPOUND_DEPTH`) must
            // not make the stack grow indefinitely.
            return;
        }
        self.timeline_stack.push(current);
        self.entered_from.push((clip, self.timeline_state.playhead));
        self.switch_to_timeline(nested_id);
        let end = self.session.project.timelines[nested_id].total_frames();
        self.timeline_state.playhead = at.clamp(0, (end - 1).max(0));
        self.timeline_state.reveal_playhead = true;
    }

    /// Goes back to the timeline at position `index` of `timeline_stack` (0 =
    /// the root): a clicked breadcrumb segment. The playhead lands where the
    /// one inside the compound clips is, level by level.
    fn exit_to_timeline_stack_index(&mut self, index: usize) {
        let Some(target) = self.timeline_stack.get(index).copied() else {
            return;
        };
        let frame = self.playhead_at_stack_level(index);
        self.timeline_stack.truncate(index);
        self.entered_from.truncate(index);
        self.switch_to_timeline(target);
        self.timeline_state.playhead = frame;
        self.timeline_state.reveal_playhead = true;
    }

    /// The playhead brought out of the compound clips to the timeline at
    /// position `index` of `timeline_stack`.
    fn playhead_at_stack_level(&self, index: usize) -> FrameIdx {
        let mut frame = self.timeline_state.playhead;
        for level in (index..self.timeline_stack.len()).rev() {
            let (clip_id, entry_playhead) = self.entered_from[level];
            let parent = &self.session.project.timelines[self.timeline_stack[level]];
            let clip = clip_id.and_then(|id| {
                parent
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .find(|c| c.id == id)
            });
            frame = match clip {
                // A freeze frame shows one frame of it for its whole length.
                Some(clip) if clip.freeze.is_none() => clip
                    .timeline_frame_at(frame)
                    .clamp(clip.timeline_start, clip.timeline_end() - 1),
                _ => entry_playhead,
            };
        }
        frame
    }

    /// The part common to the two functions above: it clears the state
    /// belonging to the level being left (selection, playhead, active clip) and
    /// immediately wakes `render_ahead` on the new timeline, ignoring the
    /// generation gate of `sync_render_ahead` (here the timeline itself
    /// changes, not its content — `sync_render_ahead` would not
    /// notice on its own). The clipboard survives: copying from one
    /// timeline/compound clip and pasting into another must work
    /// (`paste_clipboard_at_playhead` already conforms for different fps).
    fn switch_to_timeline(&mut self, timeline_id: TimelineId) {
        self.stop_voiceover();
        self.timeline_id = Some(timeline_id);
        let clipboard = std::mem::take(&mut self.timeline_state.clipboard);
        self.timeline_state = timeline_ui::TimelineState::default();
        self.timeline_state.clipboard = clipboard;
        self.active_clip = None;
        self.media_pool_state.invalidate_offline();
        if let Some(render_ahead) = &self.render_ahead {
            render_ahead.update_project(&self.session.project, timeline_id);
            self.render_ahead_generation = self.session.history.generation();
        }
    }

    /// Name to show for `id` in the breadcrumb above the timeline: that
    /// of its entry in the media pool (see `MediaItem::compound`), which is
    /// what the user sees elsewhere (e.g. "Compound Clip 2").
    fn timeline_display_name(&self, id: TimelineId) -> String {
        self.session
            .project
            .media_pool
            .values()
            .find(|m| m.compound == Some(id))
            .map(|m| file_label(&m.path))
            .unwrap_or_else(|| self.session.project.timelines[id].name.clone())
    }

    /// Sends `render_ahead` a copy of the project only when the history has
    /// changed.
    fn sync_render_ahead(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let Some(render_ahead) = &self.render_ahead else {
            return;
        };
        let generation = self.session.history.generation();
        if generation != self.render_ahead_generation {
            render_ahead.update_project(&self.session.project, timeline_id);
            self.render_ahead_generation = generation;
        }
        // At speeds >1x the playhead advances faster in real time: more margin
        // ahead is needed, and it must still fit the budget (see the docs of
        // `effective_window_secs`).
        render_ahead.set_frame_stride(self.playback_frame_stride());
        render_ahead.set_playback_speed(if self.is_timeline_playing() {
            self.playback_speed
        } else {
            0.0
        });
        render_ahead.set_keyframes_when_behind(self.settings.keyframes_when_behind);
        render_ahead.set_viewer_scale(self.viewer_scale);
        let (ahead_secs, behind_secs) = self.effective_window_secs(timeline_id);
        render_ahead.set_lookahead_secs(ahead_secs);
        render_ahead.set_behind_secs(behind_secs);
        render_ahead.set_target(self.timeline_state.playhead);
    }

    fn sync_root_timeline_media(&mut self) {
        if let Some(timeline_id) = self.timeline_id {
            self.session.sync_timeline_media(timeline_id);
        }
    }

    /// Prefetch window that actually fits the cache budget: `lookahead_secs`
    /// scaled by the playback speed and `behind_secs`, shrunk proportionally
    /// when the media playing together would not fit. Asking for more than the
    /// budget does not buy a longer buffer: the worker saturates in the middle
    /// of the window and leaves holes on one of the tracks, i.e. black flashes
    /// during playback.
    fn effective_window_secs(&self, timeline_id: TimelineId) -> (f64, f64) {
        let timeline = &self.session.project.timelines[timeline_id];
        let fps = timeline.fps.as_f64().max(1e-9);
        let wanted_ahead = self.settings.lookahead_secs * self.playback_speed;
        let playhead = self.timeline_state.playhead;
        let from = (playhead - (self.settings.behind_secs * fps).ceil() as FrameIdx).max(0);
        let to = playhead + (wanted_ahead * fps).ceil() as FrameIdx;
        let frame_bytes = self.window_frame_bytes(timeline_id, from, to).max(1);
        // Headroom: the transit frames decoded before a segment sit in the
        // cache until the next reconcile.
        let cached_per_sec = fps / self.playback_frame_stride() as f64;
        let affordable =
            self.settings.cache_budget_bytes as f64 * 0.85 / (frame_bytes as f64 * cached_per_sec);
        let wanted_total = wanted_ahead + self.settings.behind_secs;
        if wanted_total <= affordable {
            return (wanted_ahead, self.settings.behind_secs);
        }
        let scale = affordable / wanted_total;
        (
            (wanted_ahead * scale).max(MIN_LOOKAHEAD_SECS),
            self.settings.behind_secs * scale,
        )
    }

    fn playback_frame_stride(&self) -> FrameIdx {
        frame_stride(
            self.settings.fast_playback_shown_frames_only,
            self.is_timeline_playing(),
            self.playback_speed,
        )
    }

    /// Bytes one frame of each distinct media playing in `[from, to)` takes in
    /// the cache: the media inside compound clips, at the size the preview
    /// resolution decodes them to.
    fn window_frame_bytes(&self, timeline_id: TimelineId, from: FrameIdx, to: FrameIdx) -> usize {
        let shown =
            render_ahead::shown_sizes(&self.session.project, timeline_id, self.viewer_scale);
        let mut seen = std::collections::HashSet::new();
        self.frame_bytes_within(timeline_id, from, to, &shown, &mut seen, 0)
    }

    fn frame_bytes_within(
        &self,
        timeline_id: TimelineId,
        from: FrameIdx,
        to: FrameIdx,
        shown: &std::collections::HashMap<MediaId, (u32, u32)>,
        seen: &mut std::collections::HashSet<MediaId>,
        depth: u32,
    ) -> usize {
        let Some(timeline) = self.session.project.timelines.get(timeline_id) else {
            return 0;
        };
        if depth >= vv_core::MAX_COMPOUND_DEPTH {
            return 0;
        }
        let divisor = self.settings.preview_resolution.divisor().max(1);
        let mut bytes = 0;
        for (_, clip) in timeline.visible_video_clips() {
            let vv_core::ClipSource::Media(media_id) = clip.source else {
                continue;
            };
            if clip.timeline_end() <= from || clip.timeline_start >= to {
                continue;
            }
            let Some(item) = self.session.project.media_pool.get(media_id) else {
                continue;
            };
            if let Some(nested_id) = item.compound {
                let a = clip.picture_frame_at(clip.timeline_start.max(from));
                let b = clip.picture_frame_at(clip.timeline_end().min(to) - 1);
                let (start, end) = (a.min(b), a.max(b) + 1);
                bytes += self.frame_bytes_within(nested_id, start, end, shown, seen, depth + 1);
            } else if seen.insert(media_id) {
                let (w, h) = shown
                    .get(&media_id)
                    .copied()
                    .unwrap_or((item.meta.width, item.meta.height));
                bytes += vv_media::yuv420_frame_bytes((w / divisor).max(1), (h / divisor).max(1));
            }
        }
        bytes
    }

    /// Drop of an effect from the Effects panel: the generator clip goes on the
    /// video track indicated by `target`, at `start`.
    fn add_generator_to_timeline_at(
        &mut self,
        generator: timeline_ui::Generator,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        if generator == timeline_ui::Generator::Text {
            let template = self.dropped_text_template();
            self.insert_text_template(template.as_deref(), start, target);
            return;
        }
        let group = self.session.history.begin_group();
        let timeline_id = self.ensure_timeline();
        if let Some(tracks) = self.resolve_drop_tracks(timeline_id, target, true, false) {
            // A generator always has video: `resolve_drop_tracks` with
            // `any_video: true` always resolves to `Some`.
            let video_track = tracks
                .video
                .expect("generator: video track always resolved");
            vv_core::edit::insert_generator(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                generator,
                video_track,
                start,
                None,
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    fn add_drop_to_timeline_at(
        &mut self,
        drag: &timeline_ui::TimelineDrag,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        match drag {
            timeline_ui::TimelineDrag::Media(set) => {
                self.add_media_set_to_timeline_at(set, start, target)
            }
            timeline_ui::TimelineDrag::Generator(g) => {
                self.add_generator_to_timeline_at(*g, start, target)
            }
            timeline_ui::TimelineDrag::Fx(fx) => {
                self.add_fx_element_at(&fx.id, fx.millis, start, target)
            }
        }
    }

    fn insert_clips_overwriting(
        &mut self,
        timeline_id: TimelineId,
        clips: Vec<(usize, vv_core::Clip, Option<u64>)>,
        label: vv_core::CommandLabel,
    ) {
        vv_core::edit::insert_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            clips,
            label,
        );
    }

    /// Whether the auto balance can measure `targets`: a single clip, under
    /// the playhead, since it is composed there.
    fn can_auto_balance(&self, targets: &[PanelTarget]) -> bool {
        matches!(targets, [t] if self.isolation_at_playhead(t).is_some())
    }

    /// The isolation composing `target` by itself, if the playhead is on it.
    fn isolation_at_playhead(&self, target: &PanelTarget) -> Option<Isolation> {
        let clip = self.session.project.timelines[target.timeline]
            .clip(target.track_index, target.clip_id)?;
        clip.contains(self.timeline_state.playhead)
            .then_some(Isolation {
                track_index: target.track_index,
                keep_below: clip.is_adjustment(),
            })
    }

    /// Adds to `section` the wheel moves balancing `target` at the playhead,
    /// composed apart from what covers it (see `Isolation`); nothing while
    /// its frames are not decoded.
    fn balance_isolated(
        &mut self,
        section: &mut grade_panel::GradeSectionResponse,
        grade: &vv_core::GradeValue,
        target: PanelTarget,
    ) {
        if let Some(pixels) = self.isolated_pixels(target) {
            grade_panel::balance_edits(section, grade, &pixels);
        }
    }

    /// RGBA of `target` at the playhead, composed apart from what covers it.
    pub(crate) fn isolated_pixels(&mut self, target: PanelTarget) -> Option<Vec<u8>> {
        let isolation = self.isolation_at_playhead(&target)?;
        let layers = self.timeline_video_layers(Some(isolation))?;
        let output = self.viewer_output(&layers);
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        let texture = self
            .compositor
            .render_layers_to_texture(&render_layers, output);
        Some(
            self.compositor
                .read_rgba_texture(&texture, texture.width(), texture.height()),
        )
    }

    /// The frame the viewer composes `layers` into: at the resolution of the
    /// largest decoded frame, widened to the aspect of the timeline so the
    /// bars show without upscaling, and capped by the preview resolution. With SolidColor clips only, at the
    /// timeline's resolution; empty, a tiny black frame of its aspect.
    fn viewer_output(&self, layers: &[frame_provider::OwnedLayer]) -> vv_render::OutputFrame {
        let video_size = layers
            .iter()
            .filter_map(|l| match &l.content {
                frame_provider::OwnedContent::Video { frame, .. } => {
                    Some((frame.width, frame.height))
                }
                _ => None,
            })
            .reduce(|a, b| (a.0.max(b.0), a.1.max(b.1)));
        let timeline_size = self
            .timeline_id
            .map(|id| self.session.project.timelines[id].resolution);
        match video_size.or(timeline_size.filter(|_| !layers.is_empty())) {
            Some(size) => {
                let timeline_size = timeline_size.unwrap_or(size);
                let (out_w, out_h) = vv_render::fit_output_size(size, timeline_size);
                vv_render::OutputFrame::scaled(out_w, out_h, timeline_size)
                    .reduced(self.settings.preview_resolution.divisor())
            }
            None => {
                let (w, h) = timeline_size.unwrap_or((16, 9));
                let step = (w.max(h) / 64).max(1);
                vv_render::OutputFrame::scaled((w / step).max(1), (h / step).max(1), (w, h))
            }
        }
    }

    /// Measures the viewer's frame for the Color window's scope slots, shown
    /// on the next frame. Only what changed (frame, scope or size) is redrawn.
    fn update_scopes(&mut self, ctx: &egui::Context) {
        if !self.color_editor_shown() {
            self.scope_frame = None;
            return;
        }
        let source = self.scope_source();
        self.scope_view.has_source = source.is_some();
        let (Some(render_state), Some(source)) = (self.egui_render_state.clone(), source) else {
            return;
        };
        let scopes = self.scopes.get_or_insert_with(|| {
            vv_render::Scopes::new(
                std::sync::Arc::new(render_state.device.clone()),
                std::sync::Arc::new(render_state.queue.clone()),
            )
        });
        for slot in 0..2 {
            let size = self.scope_view.sizes[slot];
            if size.0 == 0 || size.1 == 0 {
                continue;
            }
            let kind = self.settings.panels.color_scopes[slot];
            let drawn = Some((kind, size, self.scope_generation));
            if self.scope_view.drawn[slot] == drawn {
                continue;
            }
            let texture = scopes.render(&source, kind, slot, size);
            // `Scopes` redraws a slot's texture in place while its size holds.
            let registered = &mut self.scope_view.textures[slot];
            if registered.as_ref().is_none_or(|(_, t)| *t != texture) {
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let mut renderer = render_state.renderer.write();
                let id = match registered {
                    Some((id, _)) => {
                        renderer.update_egui_texture_from_wgpu_texture(
                            &render_state.device,
                            &view,
                            wgpu::FilterMode::Linear,
                            *id,
                        );
                        *id
                    }
                    None => renderer.register_native_texture(
                        &render_state.device,
                        &view,
                        wgpu::FilterMode::Linear,
                    ),
                };
                *registered = Some((id, texture));
            }
            self.scope_view.drawn[slot] = drawn;
            ctx.request_repaint();
        }
    }

    /// What the scopes measure: `scope_target` composed by itself at the
    /// playhead, recomposed only when its stack changes. While its frames are
    /// not decoded, the last composition.
    fn scope_source(&mut self) -> Option<wgpu::Texture> {
        let Some(isolation) = self
            .scope_target
            .and_then(|t| self.isolation_at_playhead(&t))
        else {
            self.scope_frame = None;
            return None;
        };
        let Some(layers) = self.timeline_video_layers(Some(isolation)) else {
            return self.scope_frame.as_ref().map(|(.., t)| (**t).clone());
        };
        let output = self.viewer_output(&layers);
        if let Some((shown, shown_output, texture)) = &self.scope_frame
            && *shown_output == output
            && frame_provider::renders_same(shown, &layers)
        {
            return Some((**texture).clone());
        }
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        // Owned: a pooled output would be drawn over by the viewer's next
        // frame of the same size.
        let texture = self
            .compositor
            .render_layers_to_owned_texture_transparent(&render_layers, output);
        drop(render_layers);
        let source = (*texture).clone();
        self.scope_frame = Some((layers, output, texture));
        self.scope_generation += 1;
        Some(source)
    }

    /// Undo, redo and opening a project change it too: checked before each
    /// composition rather than where it is set.
    fn sync_compositor_precision(&mut self) {
        let precision = self.session.project.precision;
        if self.compositor.precision() != precision {
            self.compositor.set_precision(precision);
            self.viewer_content = None;
            self.scope_frame = None;
        }
    }

    /// Composes `layers` and shows the resulting texture in the viewer, without
    /// readback. It does nothing without a shared device (tests).
    fn show_composited(
        &mut self,
        layers: Vec<frame_provider::OwnedLayer>,
        output: vv_render::OutputFrame,
    ) {
        self.sync_compositor_precision();
        let unchanged = self.video_texture_id.is_some()
            && self
                .viewer_content
                .as_ref()
                .is_some_and(|(shown, shown_output)| {
                    *shown_output == output && frame_provider::renders_same(shown, &layers)
                });
        if unchanged {
            self.last_viewer_frame_kind = Some(ViewerFrameKind::Video);
            return;
        }
        let Some(render_state) = self.egui_render_state.clone() else {
            return;
        };
        let render_layers: Vec<vv_render::Layer> = layers
            .iter()
            .map(frame_provider::OwnedLayer::as_render)
            .collect();
        let texture = self
            .compositor
            .render_layers_to_texture(&render_layers, output);
        drop(render_layers);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = render_state.renderer.write();
        match self.video_texture_id {
            Some(id) => renderer.update_egui_texture_from_wgpu_texture(
                &render_state.device,
                &view,
                wgpu::FilterMode::Linear,
                id,
            ),
            None => {
                self.video_texture_id = Some(renderer.register_native_texture(
                    &render_state.device,
                    &view,
                    wgpu::FilterMode::Linear,
                ));
            }
        }
        drop(renderer);
        let (width, height) = output.timeline_size;
        self.video_display_size = Some(egui::vec2(width as f32, height as f32));
        self.last_viewer_frame_kind = Some(ViewerFrameKind::Video);
        // Not kept with a compound texture: never reusable, and holding it
        // would keep it out of the scratch pool.
        let reusable = !layers
            .iter()
            .any(|l| matches!(l.content, frame_provider::OwnedContent::Texture { .. }));
        self.viewer_content = reusable.then_some((layers, output));
    }

    /// The layers to compose at the playhead, from bottom to top. `None` if a
    /// media frame is not ready: the frame already shown is kept, composing
    /// without that layer would flash black where the clip should be. Once the
    /// buffer says it is caught up the missing frame will never arrive (media
    /// that does not decode): the layer is dropped instead of freezing the
    /// preview. During a crossing transition both halves must be ready.
    /// With `isolation`, only the tracks it shows.
    fn timeline_video_layers(
        &mut self,
        isolation: Option<Isolation>,
    ) -> Option<Vec<frame_provider::OwnedLayer>> {
        self.sync_compositor_precision();
        let timeline = &self.session.project.timelines[self.timeline_id?];
        // With keyframes standing in, a clip with none inside it has nothing
        // to show yet: the frame shown stays rather than going black.
        let still_filling = self
            .render_ahead
            .as_ref()
            .is_some_and(|r| !r.is_caught_up() || r.is_standing_in());
        let render_ahead = self.render_ahead.as_mut()?;
        // Left over by another composition: the caller reads it for this one.
        render_ahead.take_served_keyframe();
        let playhead = self.timeline_state.playhead;
        let clips = timeline.active_video_clips_at(playhead);
        let mut groups = Vec::with_capacity(clips.len());
        for &(track_index, clip) in &clips {
            if isolation.is_some_and(|i| !i.shows(track_index)) {
                continue;
            }
            let frame = playhead.max(clip.timeline_start);
            let involved = match timeline.tracks[track_index].crossing_at(frame) {
                Some((left, right, crossing)) if left.id == clip.id || right.id == clip.id => {
                    // A Smooth Cut or asset transition merges both sides into one layer.
                    if crossing.transition.merges_sides() {
                        1
                    } else {
                        2
                    }
                }
                _ => 1,
            };
            let mut provider = frame_provider::GpuCompounds::new(render_ahead, &self.compositor)
                .with_preview_divisor(self.settings.preview_resolution.divisor());
            let track_layers = frame_provider::track_layers_at(
                &self.session.project,
                timeline,
                track_index,
                clip,
                frame,
                timeline.resolution,
                &mut provider,
            )
            .ok()?;
            if track_layers.len() < involved {
                if std::env::var("VV_DEBUG_RENDER_AHEAD").is_ok()
                    && let Some((media_id, source_frame)) =
                        frame_provider::media_source_frame(clip, frame)
                {
                    eprintln!(
                        "[viewer] MISSING-LAYER playhead={playhead} track={track_index} media={media_id:?} source_frame={source_frame} caught_up={} cached_ranges={:?}",
                        !still_filling,
                        render_ahead.cached_ranges_for(media_id),
                    );
                }
                if still_filling {
                    return None;
                }
            }
            groups.push((clip.effects.matte, track_layers));
        }
        let mut provider = frame_provider::GpuCompounds::new(render_ahead, &self.compositor)
            .with_preview_divisor(self.settings.preview_resolution.divisor());
        Some(frame_provider::stack_layers(
            groups,
            &mut provider,
            timeline.resolution,
        ))
    }

    #[cfg(test)]
    fn active_clip_effects(&self) -> Option<&vv_core::EffectStack> {
        let (track_index, clip_id) = self.active_clip?;
        let timeline_id = self.timeline_id?;
        self.session.project.timelines[timeline_id]
            .clip(track_index, clip_id)
            .map(|c| &c.effects)
    }

    /// The viewer's split view during a slip: `None` when no slip is in
    /// progress, `Some(None)` while its frames decode (the frame already shown
    /// stays).
    fn slip_split_layers(&mut self) -> Option<Option<Vec<frame_provider::OwnedLayer>>> {
        let Some(preview) = self.timeline_state.slip_preview() else {
            self.slip_viewer = None;
            return None;
        };
        let item = self.session.project.media_pool.get(preview.media_id)?;
        // A compound clip renders through its nested timeline, which the
        // single-media buffer does not have.
        if item.compound.is_some() || !item.meta.has_video {
            return None;
        }
        let source_size = (item.meta.width, item.meta.height);
        let timeline_size = self.session.project.timelines[self.timeline_id?].resolution;
        if self
            .slip_viewer
            .as_ref()
            .is_none_or(|v| v.media_id() != preview.media_id)
        {
            // Only the frame under each end matters: a small slice of the budget.
            let spawn = || {
                self.spawn_media_render_ahead(
                    self.session.project.media_pool[preview.media_id].clone(),
                    self.settings.cache_budget_bytes / 8,
                    0.0,
                    0.0,
                )
            };
            self.slip_viewer = Some(slip_viewer::SlipViewer::new(
                preview.media_id,
                [spawn(), spawn()],
            ));
        }
        let frames = self.slip_viewer.as_mut()?.frames(preview);
        Some(frames.map(|f| slip_viewer::split_layers(f, source_size, timeline_size)))
    }

    /// The frame of the media pool preview at its playhead, if already
    /// decoded.
    fn browsing_video_frame(&mut self) -> Option<std::sync::Arc<vv_media::FrameYuv420>> {
        let (render_ahead, media_id) = self.browsing_render_ahead.as_ref()?;
        render_ahead.set_target(self.browse_playhead);
        render_ahead.get_frame(*media_id, self.browse_playhead)
    }

    /// Appends the media to the video track (and the audio clips alongside).
    #[cfg(test)]
    fn add_media_to_timeline(&mut self, media_id: MediaId) {
        let Some(item) = self.session.project.media_pool.get(media_id) else {
            return;
        };
        let meta = item.meta.clone();
        // If a timeline does not exist yet (e.g. first drag&drop from the media
        // pool), it is created on the fly inheriting fps/resolution from this
        // media.
        let rotation = self.media_rotation(media_id);
        let timeline_id = self.ensure_timeline_for(&meta, rotation, None);
        if self
            .session
            .project
            .would_create_a_cycle(media_id, timeline_id)
        {
            return;
        }
        let video_track = self.session.project.timelines[timeline_id]
            .first_track_index(TrackKind::Video)
            .unwrap_or(0);
        let video_start = track_end(&self.session.project, timeline_id, video_track);
        self.insert_media_clip(
            timeline_id,
            timeline_ui::MediaDrag::whole(media_id, &meta),
            &meta,
            video_start,
            TargetTracks {
                video: Some(video_track),
                extra_audio: None,
            },
        );
    }

    /// Like `add_media_to_timeline`, but places the clip at `start` (position
    /// and `target` from the drag&drop onto the timeline, see `MediaDropTarget`).
    #[cfg(test)]
    fn add_media_to_timeline_at(
        &mut self,
        drag: timeline_ui::MediaDrag,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        self.add_media_set_to_timeline_at(&timeline_ui::MediaDragSet::one(drag), start, target);
    }

    /// Drop of one or more media: appended from `start` in pool order. The
    /// new tracks are created once for the whole drop.
    fn add_media_set_to_timeline_at(
        &mut self,
        set: &timeline_ui::MediaDragSet,
        start: FrameIdx,
        target: timeline_ui::MediaDropTarget,
    ) {
        let drops: Vec<(timeline_ui::MediaDrag, vv_core::MediaMeta)> = set
            .items
            .iter()
            .filter(|d| d.source_len() > 0)
            .filter_map(|d| {
                let item = self.session.project.media_pool.get(d.media_id)?;
                Some((*d, item.meta.clone()))
            })
            .collect();
        if drops.is_empty() {
            return;
        }
        // One drop = one Ctrl+Z, even if inside there are N clips (one
        // per audio stream of each media) plus the timeline and the tracks
        // created on the fly.
        let group = self.session.history.begin_group();
        // fps and resolution from the first *video* media of the set: an audio at the
        // head has no resolution to give the timeline.
        let timeline_id = match drops.iter().find(|(d, meta)| d.takes_video(meta)) {
            Some((first, video_meta)) => {
                let rotation = self.media_rotation(first.media_id);
                self.ensure_timeline_for(video_meta, rotation, None)
            }
            None => self.ensure_timeline_audio_only(None),
        };
        // A drop that would close a cycle (a timeline imported inside
        // itself, directly or through one of its compound clips) is
        // discarded instead of corrupting the project — see
        // `Project::would_create_a_cycle`.
        let drops: Vec<(timeline_ui::MediaDrag, vv_core::MediaMeta)> = drops
            .into_iter()
            .filter(|(d, _)| {
                !self
                    .session
                    .project
                    .would_create_a_cycle(d.media_id, timeline_id)
            })
            .collect();
        if drops.is_empty() {
            self.session.history.end_group(group);
            return;
        }
        let any_video = drops.iter().any(|(d, meta)| d.takes_video(meta));
        let any_audio = drops.iter().any(|(d, meta)| d.takes_audio(meta));
        let Some(tracks) = self.resolve_drop_tracks(timeline_id, target, any_video, any_audio)
        else {
            self.session.history.end_group(group);
            return;
        };

        let timeline_fps = self.session.project.timelines[timeline_id].fps;
        let mut cursor = start;
        let main_track = self.session.project.timelines[timeline_id].main_track_index();
        if self.timeline_state.magnet && tracks.video.is_some() && tracks.video == main_track {
            let project = &self.session.project;
            cursor = vv_core::workflow::main_insert_point(project, timeline_id, start, &[]);
            let anchors = if self.timeline_state.linkage {
                vv_core::workflow::linkage_anchors(project, timeline_id)
            } else {
                Vec::new()
            };
            let total: FrameIdx = drops
                .iter()
                .map(|(drag, meta)| {
                    drag.timeline_len(vv_core::Rational::conform_rate(timeline_fps, meta.fps))
                })
                .sum();
            vv_core::workflow::open_main_gap(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                cursor,
                total,
                &anchors,
            );
        }
        for (drag, meta) in &drops {
            self.insert_media_clip(timeline_id, *drag, meta, cursor, tracks);
            let rate = vv_core::Rational::conform_rate(timeline_fps, meta.fps);
            cursor += drag.timeline_len(rate);
        }
        if self.timeline_state.magnet {
            vv_core::workflow::compact_main_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                &[],
            );
        }
        self.session
            .history
            .end_group_as(group, vv_core::CommandLabel::InsertClips);
    }

    /// Creates the tracks required by `target` (once per drop) and
    /// returns where the clips will go.
    fn resolve_drop_tracks(
        &mut self,
        timeline_id: TimelineId,
        target: timeline_ui::MediaDropTarget,
        any_video: bool,
        any_audio: bool,
    ) -> Option<TargetTracks> {
        let video = if !any_video {
            None
        } else {
            Some(match target {
                timeline_ui::MediaDropTarget::NewVideoTrack => timeline_ui::add_track(
                    &mut self.session.project,
                    &mut self.session.history,
                    timeline_id,
                    TrackKind::Video,
                ),
                timeline_ui::MediaDropTarget::Track(track) => {
                    if self.session.project.timelines[timeline_id].is_locked(track) {
                        return None;
                    }
                    track
                }
                _ => match self.session.project.timelines[timeline_id]
                    .first_unlocked_track_index(TrackKind::Video)
                {
                    Some(track) => track,
                    // No free video track: one is created.
                    None => timeline_ui::add_track(
                        &mut self.session.project,
                        &mut self.session.history,
                        timeline_id,
                        TrackKind::Video,
                    ),
                },
            })
        };
        let extra_audio = if target == timeline_ui::MediaDropTarget::NewAudioTrack && any_audio {
            Some(timeline_ui::add_track(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                TrackKind::Audio,
            ))
        } else {
            None
        };
        Some(TargetTracks { video, extra_audio })
    }

    fn insert_media_clip(
        &mut self,
        timeline_id: TimelineId,
        drag: timeline_ui::MediaDrag,
        meta: &vv_core::MediaMeta,
        start: FrameIdx,
        tracks: TargetTracks,
    ) {
        let takes_video = drag.takes_video(meta);
        let inserted = vv_core::edit::insert_media(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            vv_core::edit::MediaInsert {
                media_id: drag.media_id,
                source_in: drag.source_in,
                source_out: drag.source_out,
                video: takes_video,
                audio: drag.takes_audio(meta),
            },
            start,
            tracks,
        );
        // The video clip comes first.
        if takes_video && let Some(&clip) = inserted.as_ref().and_then(|clips| clips.first()) {
            self.upright_clip(timeline_id, clip, drag.media_id, meta);
        }
    }

    /// Transform handles of the first selected video clip, if it is under
    /// the playhead and the viewer shows the timeline stopped.
    fn open_settings(&mut self, section: settings_dialog::Section) {
        let mut dialog = settings_dialog::SettingsDialog::new(section);
        dialog.mcp_forced = self.mcp_forced;
        self.settings_dialog = Some(dialog);
    }

    fn show_settings_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.settings_dialog else {
            return;
        };
        let before = self.settings.clone();
        let response = dialog.show(ctx, &mut self.settings);
        if self.settings.mcp_enabled != before.mcp_enabled {
            self.apply_mcp_setting(ctx);
        }
        if response.gemini_key_changed {
            self.ai_clips.key_present = None;
        }
        if !response.open {
            self.settings_dialog = None;
        }
        if response.pick_models_dir {
            let start = subtitles::models_dir();
            self.spawn_file_dialog(project_io::DialogKind::ModelsFolder, move |dlg| {
                let dlg = match start {
                    Some(dir) => dlg.set_directory(dir),
                    None => dlg,
                };
                dlg.pick_folder()
            });
        }
        if self.settings.models_dir != before.models_dir {
            subtitles::set_models_dir(self.settings.models_dir.clone());
        }
        if response.pick_recording_dir {
            let start = self.settings.recording_dir.clone();
            self.spawn_file_dialog(project_io::DialogKind::RecordingFolder, move |dlg| {
                let dlg = match start {
                    Some(dir) => dlg.set_directory(dir),
                    None => dlg,
                };
                dlg.pick_folder()
            });
        }
        if !response.changed {
            return;
        }
        if self.settings.proxy() != before.proxy() {
            self.apply_proxy_settings();
        }
        vv_media::hw::set_budget_bytes(self.settings.hw_decode_budget_bytes());
        if self.settings.hw_decode != before.hw_decode {
            hw_decode::apply(self.settings.hw_decode);
            for render_ahead in self.render_aheads() {
                render_ahead.reopen_decoders();
            }
        }
        // The timeline one is then scaled on every sync (`effective_window_secs`).
        for render_ahead in self.render_aheads() {
            render_ahead.set_lookahead_secs(self.settings.lookahead_secs);
            render_ahead.set_behind_secs(self.settings.behind_secs);
            render_ahead.set_cache_budget_bytes(self.settings.cache_budget_bytes);
        }
        self.persist_settings();
    }

    /// A GPU decoder chosen explicitly that cannot open: the setting moves
    /// to CPU, Settings says why.
    fn follow_hw_decode_fallback(&mut self) {
        if let Some(fallback) = hw_decode::fallback()
            && fallback.mode != hw_decode::HwDecodeMode::Auto
            && self.settings.hw_decode == fallback.mode
        {
            self.settings.hw_decode = hw_decode::HwDecodeMode::Off;
            self.persist_settings();
        }
    }

    /// Scans off the UI thread: loading each bundle can take seconds.
    fn start_plugin_scan(&mut self, ctx: &egui::Context, manual: bool) {
        if self.plugin_scan_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let marker = self.plugin_scan_marker();
        self.settings.prepare_plugin_scan(manual, &marker);
        self.persist_settings();
        let blocklist = self.settings.blocked_paths();
        std::thread::spawn(move || {
            let _ = tx.send(vv_plugin::scan_with_guard(
                &vv_plugin::default_dirs(),
                &marker,
                &blocklist,
            ));
            ctx.request_repaint();
        });
        self.plugin_scan_rx = Some(rx);
        // A rescan keeps the complete previous list, so only a first scan gates exports.
        vv_plugin::set_scanning(self.settings.needs_plugin_scan());
    }

    /// Only while a scan runs: otherwise the marker is a real crash's, kept
    /// for the next scan.
    fn abandon_plugin_scan(&self) {
        if self.plugin_scan_rx.is_some() {
            vv_plugin::abandon_scan(&self.plugin_scan_marker());
        }
    }

    fn plugin_scan_marker(&self) -> PathBuf {
        self.settings_path
            .as_ref()
            .map(|p| p.with_file_name("vst3-scan.marker"))
            .unwrap_or_else(|| std::env::temp_dir().join("venturi-vst3-scan.marker"))
    }

    fn poll_plugin_scan(&mut self) {
        let Some(rx) = &self.plugin_scan_rx else {
            return;
        };
        let list = match rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.plugin_scan_rx = None;
                vv_plugin::set_scanning(false);
                return;
            }
        };
        self.plugin_scan_rx = None;
        let unchanged = apply_scan_result(&mut self.settings, list);
        vv_plugin::set_scanning(false);
        let group_open = self.undo_group_open();
        if let Some(audio) = &mut self.timeline_audio {
            if unchanged {
                audio.plugins_rescan_unchanged();
            } else {
                let dropped = audio.plugins_rescanned();
                self.plugin_editors.close_instances(
                    &mut self.session.project,
                    &mut self.session.history,
                    &dropped,
                    group_open,
                );
            }
        }
        self.persist_settings();
    }

    /// User settings on disk, including the panel layout: called
    /// from the Settings window and periodically/on close (see
    /// `eframe::App::save`).
    pub(crate) fn persist_settings(&mut self) {
        self.settings.workspace = self.workspace.to_value();
        if let Some(path) = &self.settings_path
            && let Err(e) = self.settings.save(path)
        {
            self.project_error = Some(t!("settings.save_failed", error = e).into_owned());
        }
    }

    /// `(total, playhead, (in, out), playing)` of the playback bar.
    fn transport_fps(&self) -> f64 {
        if self.browsing.is_some() {
            self.browse_fps()
        } else {
            self.timeline_id
                .map_or(1.0, |id| self.session.project.timelines[id].fps.as_f64())
        }
    }

    fn player_options(&self) -> transport::PlayerOptions {
        let ratio = self.timeline_id.map_or(first_media::Ratio::Original, |id| {
            first_media::Ratio::of(self.session.project.timelines[id].resolution)
        });
        let platforms = &self.safe_zone_platforms.0;
        let shorts = &self.settings.shorts;
        let chosen = safe_zones::platform(platforms, &shorts.safe_zone_platform)
            .and_then(|chosen| platforms.iter().position(|p| p == chosen));
        transport::PlayerOptions {
            ratio,
            quality: self.settings.preview_resolution,
            safe_zones: chosen.filter(|_| shorts.safe_zones),
            safe_zone_platforms: platforms
                .iter()
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect(),
        }
    }

    /// Size of the first video or photo on the timeline, as shown upright.
    fn original_resolution(&mut self, timeline_id: TimelineId) -> Option<(u32, u32)> {
        let media_id = self.session.project.timelines[timeline_id]
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .filter_map(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => Some((clip.timeline_start, id)),
                _ => None,
            })
            .filter(|(_, id)| {
                self.session
                    .project
                    .media_pool
                    .get(*id)
                    .is_some_and(|m| m.meta.has_video)
            })
            .min_by_key(|(start, _)| *start)
            .map(|(_, id)| id)?;
        let meta = self.session.project.media_pool.get(media_id)?.meta.clone();
        let rotation = self.media_rotation(media_id);
        Some(first_media::effective_size(
            (meta.width, meta.height),
            rotation,
        ))
    }

    /// One undo step; the Player's «Rapporto» and the project properties both go here.
    pub(crate) fn set_project_ratio(&mut self, ratio: first_media::Ratio) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let current = self.session.project.timelines[timeline_id].resolution;
        let original = self.original_resolution(timeline_id);
        let resolution = ratio.resolution(current, original);
        if resolution != current {
            self.session
                .apply(Box::new(vv_core::SetTimelineResolution::new(
                    timeline_id,
                    resolution,
                )));
        }
    }

    /// One undo step that keeps every clip where it is in seconds.
    pub(crate) fn set_project_fps(&mut self, fps: vv_core::Rational) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let from = self.session.project.timelines[timeline_id].fps;
        if from == fps {
            return;
        }
        self.session
            .apply(Box::new(vv_core::SetTimelineFps::new(timeline_id, fps)));
        let convert = |frame| vv_core::convert_frames(frame, from, fps);
        self.timeline_state.playhead =
            vv_core::convert_frames(self.timeline_state.playhead, from, fps);
        let marks = &mut self.timeline_state.export_marks;
        marks.mark_in = marks.mark_in.map(convert);
        marks.mark_out = marks.mark_out.map(convert);
        self.ensure_active_clip_matches_playhead(true);
    }

    pub(crate) fn set_project_resolution(&mut self, resolution: (u32, u32)) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if self.session.project.timelines[timeline_id].resolution != resolution {
            self.session
                .apply(Box::new(vv_core::SetTimelineResolution::new(
                    timeline_id,
                    resolution,
                )));
        }
    }

    fn transport_state(&self) -> (FrameIdx, FrameIdx, (FrameIdx, FrameIdx), bool) {
        let playing = self.is_timeline_playing();
        if self.browsing.is_some() {
            let total = self.browse_total_frames();
            (
                total,
                self.browse_playhead,
                self.browse_marks.resolve(total),
                playing,
            )
        } else {
            let total = self
                .timeline_id
                .map_or(0, |id| self.session.project.timelines[id].total_frames());
            (
                total,
                self.timeline_state.playhead,
                self.timeline_state.export_marks.resolve(total),
                playing,
            )
        }
    }

    /// Full-screen player over the rest of the interface, which stays
    /// drawn underneath (and keeps handling the shortcuts) but no longer
    /// receives the mouse. The playback bar appears only with the mouse at the bottom.
    fn show_fullscreen_viewer(&mut self, ctx: &egui::Context) -> transport::TransportResponse {
        const BAR_PADDING: egui::Vec2 = egui::vec2(24.0, 12.0);
        let screen = ctx.content_rect();
        let bar_zone =
            transport::HEIGHT + ctx.global_style().spacing.item_spacing.y + 2.0 * BAR_PADDING.y;
        let (total, playhead, marks, playing) = self.transport_state();
        let fps = self.transport_fps();
        let options = self.player_options();
        let mut action = transport::TransportResponse::default();
        egui::Area::new(egui::Id::new("fullscreen_viewer"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.interact(screen, ui.id().with("block"), egui::Sense::click_and_drag());
                ui.painter().rect_filled(screen, 0.0, egui::Color32::BLACK);

                let image = match self.last_viewer_frame_kind {
                    Some(ViewerFrameKind::Video) => {
                        self.video_texture_id.zip(self.video_display_size)
                    }
                    Some(ViewerFrameKind::Offline) | None => None,
                };
                if let Some((id, size)) = image {
                    let scale = (screen.width() / size.x).min(screen.height() / size.y);
                    let rect = egui::Rect::from_center_size(screen.center(), size * scale.max(0.0));
                    ui.painter().image(
                        id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    if self.viewer_keyframe {
                        paint_keyframes_badge(ui.painter(), rect);
                    }
                } else if self.last_viewer_frame_kind == Some(ViewerFrameKind::Offline) {
                    ui.painter().text(
                        screen.center(),
                        egui::Align2::CENTER_CENTER,
                        t!("viewer.media_offline"),
                        egui::FontId::proportional(24.0),
                        theme::ERROR,
                    );
                }

                let hovering_bar = ui
                    .input(|i| i.pointer.hover_pos())
                    .is_some_and(|p| p.y >= screen.bottom() - bar_zone);
                if hovering_bar {
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(screen.left(), screen.bottom() - bar_zone),
                        screen.max,
                    );
                    ui.painter()
                        .rect_filled(bar, 0.0, egui::Color32::from_black_alpha(170));
                    let inner = bar.shrink2(BAR_PADDING);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                        action = transport::show_transport(
                            ui, total, playhead, marks, playing, fps, options,
                        );
                    });
                }
            });
        action
    }

    fn show_viewer_zoom_bar(&mut self, ui: &mut egui::Ui) {
        let ppp = ui.ctx().pixels_per_point();
        let geometry = self
            .viewer_geometry
            .filter(|_| self.last_viewer_frame_kind == Some(ViewerFrameKind::Video));
        let label = match geometry {
            Some((area, frame_px)) => {
                viewer_zoom::percent_label(self.viewer_zoom.scale(area, frame_px, ppp))
            }
            None => "–".to_owned(),
        };
        let keymap = &self.settings.keymap;
        let shortcut_text = |action| {
            keymap
                .shortcuts(action)
                .first()
                .map(ToString::to_string)
                .unwrap_or_default()
        };
        let title = self
            .timeline_id
            .map(|id| t!("player.title", name = self.timeline_display_name(id)).into_owned());
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), 40.0 - theme::SPACE_2),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.add_enabled_ui(geometry.is_some(), |ui| {
                    ui.menu_button("≡", |ui| {
                        let Some((area, frame_px)) = geometry else {
                            return;
                        };
                        ui.weak(&label);
                        let fit = egui::Button::selectable(
                            self.viewer_zoom.is_fit(),
                            t!("viewer.zoom_fit"),
                        )
                        .shortcut_text(shortcut_text(Action::ViewerZoomFit));
                        if ui.add(fit).clicked() {
                            self.viewer_zoom.fit();
                            ui.close();
                        }
                        ui.separator();
                        let current = self.viewer_zoom.scale(area, frame_px, ppp);
                        for preset in viewer_zoom::PRESETS {
                            let percent = viewer_zoom::percent_label(preset);
                            let button = if preset == 1.0 {
                                egui::Button::selectable(
                                    false,
                                    t!("viewer.zoom_actual", percent = percent),
                                )
                                .shortcut_text(shortcut_text(Action::ViewerZoomActual))
                            } else {
                                egui::Button::selectable(false, percent)
                            };
                            let selected =
                                !self.viewer_zoom.is_fit() && (current - preset).abs() < 1e-4;
                            if ui.add(button.selected(selected)).clicked() {
                                self.viewer_zoom.set_scale(preset, area, frame_px, ppp);
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text(t!("viewer.zoom_hint"));
                });
                if let Some(title) = title {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(title).selectable(false).truncate());
                    });
                }
            },
        );
    }

    fn show_viewer_overlay(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        area: egui::Rect,
        video_targets: &[PanelTarget],
        pending: &mut Vec<BoxedCommand>,
    ) {
        let visible = self.show_transform_overlay
            && self.browsing.is_none()
            && !self.is_timeline_playing()
            && matches!(self.last_viewer_frame_kind, Some(ViewerFrameKind::Video));
        let Some((timeline_id, target)) = self.timeline_id.zip(video_targets.first().copied())
        else {
            self.overlay_drag = None;
            return;
        };
        let playhead = self.timeline_state.playhead;
        let under_playhead = self.session.project.timelines[timeline_id]
            .clip(target.track_index, target.clip_id)
            .is_some_and(|c| c.contains(playhead));
        let Some(info) = self
            .clip_panel_info(target)
            .filter(|_| visible && under_playhead)
        else {
            self.overlay_drag = None;
            return;
        };
        let clip =
            self.session.project.timelines[timeline_id].clip(target.track_index, target.clip_id);
        let focused_mask = self
            .mask_focus
            .filter(|(id, _)| *id == target.clip_id)
            .and_then(|(_, index)| Some((index, clip?.effects.masks.get(index)?)));
        if let Some((index, mask)) = focused_mask {
            self.overlay_drag = None;
            if let Some(edited) = mask_overlay::show(
                ui,
                rect,
                area,
                info.timeline_size,
                &info.transform,
                mask,
                target.source_frame,
                &mut self.mask_overlay,
            ) {
                let mut masks = clip.map(|c| c.effects.masks.clone()).unwrap_or_default();
                masks[index] = edited;
                pending.push(Box::new(vv_core::set_clip_masks(
                    timeline_id,
                    target.track_index,
                    target.clip_id,
                    masks,
                )));
            }
            return;
        }
        let content = info
            .title
            .as_ref()
            .and_then(|title| title_content_box(title, info.timeline_size));
        let Some(new) = viewer_overlay::show(
            ui,
            rect,
            area,
            info.timeline_size,
            info.source_size,
            content,
            &info.transform,
            &mut self.overlay_drag,
        ) else {
            return;
        };
        push_param_changes(
            pending,
            Some(&self.session.project.timelines[timeline_id]),
            video_targets,
            &vv_core::TransformParam::ALL,
            &new,
            &info.transform,
        );
    }

    /// D: disables the selected clips, or re-enables them if they are all already disabled.
    fn toggle_disabled_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let selected: Vec<(usize, ClipId)> = self
            .timeline_state
            .selected
            .iter()
            .copied()
            .filter(|&(track_index, _)| !tl.is_locked(track_index))
            .collect();
        if selected.is_empty() {
            return;
        }
        let all_disabled = selected.iter().all(|&(track_index, clip_id)| {
            tl.clip(track_index, clip_id).is_some_and(|c| c.disabled)
        });
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::SetClipsDisabled::new(
                timeline_id,
                selected,
                !all_disabled,
            )),
        );
    }

    /// Ctrl+A: selects all the clips of the timeline.
    fn select_all_clips(&mut self) {
        self.select_clips(|_| true);
    }

    /// Ctrl+A with the media pool focused: selects all the media of the pool.
    fn select_all_media(&mut self) {
        let ids: Vec<MediaId> = self.session.project.media_pool.keys().collect();
        self.media_pool_state.select_only(ids);
    }

    /// Alt+Y: selects from the playhead forward — the clip under the
    /// playhead is included, those ending earlier stay out.
    fn select_clips_from_playhead(&mut self) {
        let playhead = self.timeline_state.playhead;
        self.select_clips(|clip| clip.timeline_end() > playhead);
    }

    /// Selects the clips of the unlocked tracks for which `keep` is true.
    fn select_clips(&mut self, keep: impl Fn(&vv_core::Clip) -> bool) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let selected: BTreeSet<(usize, ClipId)> = self.session.project.timelines[timeline_id]
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| !track.locked)
            .flat_map(|(track_index, track)| {
                track
                    .clips
                    .iter()
                    .filter(|clip| keep(clip))
                    .map(move |clip| (track_index, clip.id))
            })
            .collect();
        let anchor = selected.iter().next().copied();
        self.timeline_state.set_selection(selected, anchor);
    }

    /// Del in the keyframe editor: it touches the keyframes, not the clip.
    fn delete_selected_keyframes(&mut self) {
        let commands = self.keyframe_editor.remove_selected(self.zoom_link);
        self.apply_effect_changes(commands, false);
    }

    fn delete_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if let Some(sel) = self.timeline_state.selected_transition {
            let cmd = set_transition_command(&self.session.project, timeline_id, sel, None);
            self.session
                .history
                .do_command(&mut self.session.project, cmd);
            self.timeline_state.selected_transition = None;
            return;
        }
        if self.timeline_state.selected.is_empty() {
            return;
        }
        let selected: Vec<(usize, ClipId)> = self.timeline_state.selected.iter().copied().collect();
        self.delete_clips_with_magnet(timeline_id, &selected);
        self.timeline_state.clear_selection();
        self.sync_selection_to_playhead();
    }

    /// Del/Backspace on the media pool: removes the selected media. The clips
    /// using them stay on the timeline and become offline (see
    /// `vv_core::RemoveMedia`).
    fn delete_selected_media(&mut self) {
        if self.media_pool_state.selected.is_empty() {
            return;
        }
        let commands: Vec<Box<dyn vv_core::Command>> = self
            .media_pool_state
            .selected
            .iter()
            .copied()
            .map(|id| Box::new(vv_core::RemoveMedia::new(id)) as Box<dyn vv_core::Command>)
            .collect();
        if matches!(&self.browsing, Some(Browsed::Media(id)) if self.media_pool_state.selected.contains(id))
        {
            self.stop_browsing();
        }
        self.session.history.do_command(
            &mut self.session.project,
            Box::new(vv_core::CompositeCommand::new(
                vv_core::CommandLabel::RemoveMedia,
                commands,
            )),
        );
        self.media_pool_state.clear();
        self.leave_removed_timelines();
    }

    pub(crate) fn undo(&mut self) {
        self.session.history.undo(&mut self.session.project);
        self.forget_removed_entities();
    }

    pub(crate) fn redo(&mut self) {
        self.session.history.redo(&mut self.session.project);
        self.forget_removed_entities();
    }

    pub(crate) fn go_to_history(&mut self, position: usize) {
        self.session
            .history
            .go_to(&mut self.session.project, position);
        self.forget_removed_entities();
    }

    /// Drops the UI's references to timelines, media and folders gone from
    /// the project, e.g. by undoing the import or the timeline that
    /// created them.
    fn forget_removed_entities(&mut self) {
        if self.browsing.is_some() && self.browsing_item().is_none() {
            self.stop_browsing();
        }
        self.leave_removed_timelines();
        self.media_pool_state.forget_removed(&self.session.project);
    }

    /// Deleting a compound clip deletes its nested timeline
    /// (`vv_core::RemoveMedia`): if it was being edited, one goes back up to the
    /// upper level that still exists.
    fn leave_removed_timelines(&mut self) {
        while self
            .timeline_id
            .is_some_and(|id| !self.session.project.timelines.contains_key(id))
        {
            match self.timeline_stack.pop() {
                Some(parent) => {
                    let (_, entry_playhead) = self.entered_from.pop().unwrap_or_default();
                    self.switch_to_timeline(parent);
                    self.timeline_state.playhead = entry_playhead;
                }
                None => {
                    self.timeline_id = None;
                    self.timeline_state = timeline_ui::TimelineState::default();
                    self.active_clip = None;
                    return;
                }
            }
        }
    }

    /// The active clip points at a media no longer in the media pool, or
    /// whose file is missing.
    fn active_clip_media_offline(&self) -> bool {
        if self.browsing.is_some() {
            return false;
        }
        let (Some(timeline_id), Some((track_index, clip_id))) =
            (self.timeline_id, self.active_clip)
        else {
            return false;
        };
        self.session.project.timelines[timeline_id]
            .clip(track_index, clip_id)
            .is_some_and(|c| match &c.source {
                vv_core::ClipSource::Media(id) => {
                    !self.session.project.media_pool.contains_key(*id)
                        || self.media_pool_state.is_offline(id)
                }
                vv_core::ClipSource::SolidColor
                | vv_core::ClipSource::Text
                | vv_core::ClipSource::Adjustment => false,
            })
    }

    /// Copy/cut/paste from the keyboard. A copy also writes the copied clips
    /// or media to the system clipboard as text, so another instance can
    /// paste them; an empty `Paste` may be an image in the clipboard.
    fn handle_clipboard_events(&mut self, ui: &egui::Ui, events: &[egui::Event]) {
        for event in events {
            match event {
                egui::Event::Copy => {
                    let copied = if self.media_pool_state.focused {
                        self.copy_selected_media()
                    } else {
                        self.copy_selected_clips()
                    };
                    if copied {
                        self.publish_clipboard(ui.ctx());
                    }
                }
                // Ctrl+X in a text field cuts the text, not the clips.
                egui::Event::Cut if !ui.ctx().egui_wants_keyboard_input() => {
                    if self.timeline_state.selected.is_empty() {
                        continue;
                    }
                    self.copy_selected_clips();
                    self.publish_clipboard(ui.ctx());
                    self.delete_selected();
                }
                egui::Event::Paste(text)
                    if !ui.ctx().egui_wants_keyboard_input()
                        && self.paste_download_link(ui.ctx(), text) => {}
                egui::Event::Paste(text) if text.is_empty() => {
                    self.request_clipboard_image(ui.ctx())
                }
                egui::Event::Paste(text) => self.paste_text(text),
                _ => {}
            }
        }
    }

    fn clipboard_origin(&self) -> vv_core::clipboard::ClipboardOrigin {
        vv_core::clipboard::ClipboardOrigin {
            instance: self.clipboard_instance,
            epoch: self.session.epoch(),
        }
    }

    fn publish_clipboard(&self, ctx: &egui::Context) {
        ctx.copy_text(self.clipboard_text());
    }

    fn clipboard_text(&self) -> String {
        vv_core::clipboard::ClipboardPayload::new(
            self.clipboard_origin(),
            &self.session.project,
            &self.copied_media,
            &self.timeline_state.clipboard,
        )
        .to_text()
    }

    fn copy_selected_media(&mut self) -> bool {
        if self.media_pool_state.selected.is_empty() {
            return false;
        }
        self.copied_media = self.media_pool_state.selected.iter().copied().collect();
        self.timeline_state.clipboard.clear();
        true
    }

    /// Text from the system clipboard: clips or media copied by Tandem,
    /// maybe another instance or project. Any other text pastes what this
    /// instance copied last.
    fn paste_text(&mut self, text: &str) {
        use vv_core::clipboard::{ClipboardPayload, PayloadError};
        let payload = match ClipboardPayload::from_text(text) {
            None => {
                if !self.media_pool_state.focused {
                    self.paste_clipboard_at_playhead();
                }
                return;
            }
            Some(Err(PayloadError::UnsupportedVersion(version))) => {
                self.project_error =
                    Some(t!("menu.paste_unsupported", version = version).into_owned());
                return;
            }
            Some(Err(PayloadError::Malformed(e))) => {
                self.project_error = Some(t!("menu.paste_malformed", error = e).into_owned());
                return;
            }
            Some(Ok(payload)) => payload,
        };
        let to_pool = self.media_pool_state.focused || !payload.has_clips();
        if payload.origin == self.clipboard_origin() {
            if to_pool {
                let media: Vec<MediaId> = self
                    .copied_media
                    .iter()
                    .copied()
                    .filter(|&id| self.session.project.media_pool.contains_key(id))
                    .collect();
                self.media_pool_state.select_only(media);
            } else {
                self.paste_clipboard_at_playhead();
            }
            return;
        }
        if to_pool {
            if !payload.has_media() {
                return;
            }
            let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::PasteMedia);
            let pasted = payload.merge_into(&mut self.session.project, &mut add);
            self.add_pasted_entities(add);
            self.media_pool_state.select_only(pasted.media);
            return;
        }
        if self.timeline_id.is_none() {
            return;
        }
        let mark = self.session.history.begin_group();
        let mut add = vv_core::AddEntities::new(vv_core::CommandLabel::PasteClips);
        let pasted = payload.merge_into(&mut self.session.project, &mut add);
        self.add_pasted_entities(add);
        self.paste_entries_at_playhead(pasted.clips);
        self.session
            .history
            .end_group_as(mark, vv_core::CommandLabel::PasteClips);
    }

    /// Pasted media need their thumbnail, proxy and waveform like imported
    /// ones. Nothing new (every file already in the pool) leaves no step.
    fn add_pasted_entities(&mut self, add: vv_core::AddEntities) {
        if add.is_empty() {
            return;
        }
        let added: Vec<MediaId> = add.media_ids().collect();
        self.session.apply(Box::new(add));
        for media_id in added {
            self.enqueue_media_background_jobs(media_id);
        }
    }

    /// Copies the selected clips (the linked groups are all already inside the
    /// selection).
    fn copy_selected_clips(&mut self) -> bool {
        let Some(timeline_id) = self.timeline_id else {
            return false;
        };
        if self.timeline_state.selected.is_empty() {
            return false;
        }

        let tl = &self.session.project.timelines[timeline_id];
        // The groups are remapped onto local tags: the paste needs new groups.
        let mut collected: Vec<(Option<vv_core::LinkGroupId>, timeline_ui::ClipboardEntry)> = self
            .timeline_state
            .selected
            .iter()
            .filter_map(|&(track_index, clip_id)| {
                let clip = tl.clip(track_index, clip_id)?;
                Some((
                    clip.linked_group,
                    timeline_ui::ClipboardEntry {
                        track_kind: tl.tracks[track_index].kind,
                        track_number: tl.track_number(track_index),
                        relative_start: clip.timeline_start,
                        clip: clip.clone(),
                        timeline_fps: tl.fps,
                        link_tag: None,
                    },
                ))
            })
            .collect();

        if collected.is_empty() {
            return false;
        }

        let anchor = collected
            .iter()
            .map(|(_, e)| e.relative_start)
            .min()
            .unwrap_or(0);
        for (_, e) in &mut collected {
            e.relative_start -= anchor;
        }

        let mut tag_of: std::collections::HashMap<vv_core::LinkGroupId, u64> =
            std::collections::HashMap::new();
        for (group, e) in &mut collected {
            if let Some(g) = group {
                let next_tag = tag_of.len() as u64;
                e.link_tag = Some(*tag_of.entry(*g).or_insert(next_tag));
            }
        }

        self.timeline_state.clipboard = collected.into_iter().map(|(_, e)| e).collect();
        self.copied_media.clear();
        true
    }

    /// Pastes the clipboard at the playhead preserving the distances and the linked
    /// groups; the pasted clips overwrite what was underneath.
    fn paste_clipboard_at_playhead(&mut self) {
        self.paste_entries_at_playhead(self.timeline_state.clipboard.clone());
    }

    fn paste_entries_at_playhead(&mut self, entries: Vec<timeline_ui::ClipboardEntry>) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if entries.is_empty() {
            return;
        }
        let playhead = self.timeline_state.playhead;
        let entries: Vec<timeline_ui::ClipboardEntry> = entries
            .into_iter()
            // Pasting a copied compound clip inside its own
            // nested timeline (directly or through another
            // compound clip) would close a cycle — see
            // `Project::would_create_a_cycle`, the same check as the drop
            // from the media pool.
            .filter(|e| match &e.clip.source {
                vv_core::ClipSource::Media(media_id) => !self
                    .session
                    .project
                    .would_create_a_cycle(*media_id, timeline_id),
                vv_core::ClipSource::SolidColor
                | vv_core::ClipSource::Text
                | vv_core::ClipSource::Adjustment => true,
            })
            .collect();
        if entries.is_empty() {
            return;
        }

        let mark = self.session.history.begin_group();
        self.add_tracks_for_clipboard(timeline_id, &entries);

        let tl = &self.session.project.timelines[timeline_id];
        let entries: Vec<(usize, timeline_ui::ClipboardEntry)> = entries
            .into_iter()
            .filter_map(|e| {
                let index = tl.track_of_kind_numbered(e.track_kind, e.track_number)?;
                (!tl.is_locked(index)).then_some((index, e))
            })
            .collect();
        if entries.is_empty() {
            self.session.history.end_group(mark);
            return;
        }

        let timeline_fps = self.session.project.timelines[timeline_id].fps;
        let clips: Vec<(usize, vv_core::Clip, Option<u64>)> = entries
            .iter()
            .map(|(track_index, entry)| {
                let mut clip = entry.clip.clone();
                clip.id = self.session.project.alloc_clip_id();
                clip.timeline_start = entry.relative_start;
                clip.linked_group = None;
                if entry.timeline_fps != timeline_fps {
                    // A media gone from the pool keeps the conform rate it had.
                    let conform_rate = match &clip.source {
                        vv_core::ClipSource::Media(media_id) => self
                            .session
                            .project
                            .media_pool
                            .get(*media_id)
                            .map_or(clip.conform_rate(), |item| {
                                vv_core::Rational::conform_rate(timeline_fps, item.meta.fps)
                            }),
                        vv_core::ClipSource::SolidColor
                        | vv_core::ClipSource::Text
                        | vv_core::ClipSource::Adjustment => clip.conform_rate(),
                    };
                    clip.retime(entry.timeline_fps, timeline_fps, conform_rate);
                }
                clip.timeline_start += playhead;
                (*track_index, clip, entry.link_tag)
            })
            .collect();
        let new_selection: BTreeSet<(usize, ClipId)> = clips
            .iter()
            .map(|(track, clip, _)| (*track, clip.id))
            .collect();
        let end = clips.iter().map(|(_, clip, _)| clip.timeline_end()).max();
        self.insert_clips_overwriting(timeline_id, clips, vv_core::CommandLabel::PasteClips);
        self.session
            .history
            .end_group_as(mark, vv_core::CommandLabel::PasteClips);
        let anchor = new_selection.iter().next().copied();
        self.timeline_state.set_selection(new_selection, anchor);
        if let Some(end) = end {
            self.timeline_state.playhead = end;
            self.ensure_active_clip_matches_playhead(true);
        }
    }

    /// Tracks missing to paste `entries`: a compound clip with video
    /// tracks only, pasted into a timeline having a single one, must
    /// create its own V2, not end up on the audio.
    fn add_tracks_for_clipboard(
        &mut self,
        timeline_id: TimelineId,
        entries: &[timeline_ui::ClipboardEntry],
    ) {
        for kind in [TrackKind::Video, TrackKind::Audio] {
            let wanted = entries
                .iter()
                .filter(|e| e.track_kind == kind)
                .map(|e| e.track_number)
                .max()
                .unwrap_or(0);
            let existing = self.session.project.timelines[timeline_id]
                .tracks_of_kind(kind)
                .count();
            for _ in existing..wanted {
                self.session.history.do_command(
                    &mut self.session.project,
                    Box::new(vv_core::AddTrack::new(timeline_id, kind)),
                );
            }
        }
    }

    /// Ripple delete: removes the selected clips (and their linked
    /// groups) and closes the holes on all the tracks, preserving the A/V sync.
    fn ripple_delete_selected(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        if self.timeline_state.selected.is_empty() {
            // No clip selected: the selected gap is closed, if there is one.
            if let Some((_, gap_start, gap_end)) = self.timeline_state.selected_gap {
                vv_core::edit::ripple_delete_gap(
                    &mut self.session.project,
                    &mut self.session.history,
                    timeline_id,
                    gap_start,
                    gap_end,
                );
                self.move_playhead_to_closed_gap(timeline_id, gap_start);
                self.timeline_state.clear_selection();
                self.sync_selection_to_playhead();
            }
            return;
        }
        let selected: Vec<(usize, ClipId)> = self.timeline_state.selected.iter().copied().collect();

        let leftmost_removed = vv_core::edit::ripple_delete_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            &selected,
        );
        if let Some(position) = leftmost_removed {
            self.move_playhead_to_closed_gap(timeline_id, position);
        }
        self.timeline_state.clear_selection();
        self.sync_selection_to_playhead();
    }

    /// Brings the playhead where the clip that slid in to close the hole now starts;
    /// if none arrived it stays where it is.
    fn move_playhead_to_closed_gap(
        &mut self,
        timeline_id: vv_core::TimelineId,
        position: FrameIdx,
    ) {
        let landed = self.session.project.timelines[timeline_id]
            .tracks
            .iter()
            .any(|t| t.clips.iter().any(|c| c.timeline_start == position));
        if landed {
            self.timeline_state.playhead = position;
            self.timeline_state.reveal_playhead = true;
            self.ensure_active_clip_matches_playhead(true);
        }
    }

    fn add_marker_at_playhead(&mut self) {
        if let Some(timeline_id) = self.timeline_id {
            timeline_ui::add_marker_at_playhead(
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                self.timeline_state.playhead,
            );
        }
    }

    /// Cuts at the playhead the selected clips covering it or, without a
    /// selection, all of them. When the selection is already cut there,
    /// cuts every other clip under the playhead instead.
    fn split_at_playhead(&mut self) {
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let playhead = self.timeline_state.playhead;
        let selected = &self.timeline_state.selected;
        let targets: Vec<(usize, ClipId)> = vv_core::edit::split_clips(
            &mut self.session.project,
            &mut self.session.history,
            timeline_id,
            playhead,
            (!selected.is_empty()).then_some(selected),
        )
        .into_iter()
        .map(|(left, _)| left)
        .collect();

        if targets.is_empty() {
            let timeline = &self.session.project.timelines[timeline_id];
            let selection_cut_here = selected.iter().any(|&(track_index, clip_id)| {
                timeline.clip(track_index, clip_id).is_some_and(|clip| {
                    clip.timeline_start == playhead || clip.timeline_end() == playhead
                })
            });
            if selection_cut_here {
                vv_core::edit::split_clips(
                    &mut self.session.project,
                    &mut self.session.history,
                    timeline_id,
                    playhead,
                    None,
                );
            }
            return;
        }

        // Selects the *left* half (the one under the playhead would be the
        // right one): after a cut one usually works on what comes before.
        if self.selection_follows_playhead
            && let Some((video_track, video_clip_id)) = targets
                .iter()
                .filter(|(track_index, _)| {
                    self.session.project.timelines[timeline_id].tracks[*track_index].kind
                        == TrackKind::Video
                })
                .max_by_key(|(track_index, _)| *track_index)
        {
            let mut selected = BTreeSet::from([(*video_track, *video_clip_id)]);
            selected.extend(
                self.session.project.timelines[timeline_id]
                    .linked_members(*video_track, *video_clip_id),
            );
            self.timeline_state
                .set_selection(selected, Some((*video_track, *video_clip_id)));
        }
    }
}

/// Cache intervals in source frames → timeline frames of `clip`,
/// limited to its trim.
fn map_source_ranges_to_timeline(
    clip: &vv_core::Clip,
    source_ranges: &[(FrameIdx, FrameIdx)],
) -> Vec<(FrameIdx, FrameIdx)> {
    if let Some(frozen) = clip.freeze {
        let cached = source_ranges
            .iter()
            .any(|&(start, end)| (start..=end).contains(&frozen));
        return match cached {
            true => vec![(clip.timeline_start, clip.timeline_end() - 1)],
            false => Vec::new(),
        };
    }
    source_ranges
        .iter()
        .filter_map(|&(s_start, s_end)| {
            let start = s_start.max(clip.source_in());
            let end = s_end.min(clip.source_out() - 1);
            (start <= end).then(|| (clip.timeline_frame_at(start), clip.timeline_frame_at(end)))
        })
        .collect()
}

/// Peaks of a compound clip, composed from those of the audio clips of
/// its nested timeline: there is no file to decode, so the
/// worker cannot generate them. The `bool` is `false` if some source was
/// not cached — the waveform is partial and must be redone later.
fn compose_compound_waveform(
    project: &vv_core::Project,
    cache: &HashMap<(u64, usize), vv_media::Waveform>,
    media_id: vv_core::MediaId,
) -> Option<(vv_media::Waveform, bool)> {
    let item = project.media_pool.get(media_id)?;
    let timeline = project.timelines.get(item.compound?)?;
    let fps = timeline.fps.as_f64();
    let duration_secs = item.meta.duration_frames as f64 / fps;
    if duration_secs <= 0.0 {
        return None;
    }
    let num_peaks = vv_media::recommended_num_peaks(duration_secs);
    let mut peaks = vec![0.0f32; num_peaks];
    let mut complete = true;
    for (_, track) in timeline.audible_tracks() {
        for clip in track
            .clips
            .iter()
            .filter(|c| !c.disabled && c.freeze.is_none())
        {
            let vv_core::ClipSource::Media(source_id) = clip.source else {
                continue;
            };
            let Some(source) = project.media_pool.get(source_id) else {
                continue;
            };
            let Some(source_wf) = cache.get(&(source.content_hash, clip.audio_stream_index)) else {
                complete = false;
                continue;
            };
            if source_wf.peaks.is_empty() || source_wf.audio_duration_secs <= 0.0 {
                continue;
            }
            let bin_of = |frame: FrameIdx| {
                ((frame as f64 / fps) / duration_secs * num_peaks as f64) as usize
            };
            let first = bin_of(clip.timeline_start).min(num_peaks);
            let last = (bin_of(clip.timeline_end()) + 1).min(num_peaks);
            for (offset, peak) in peaks[first..last].iter_mut().enumerate() {
                let bin = first + offset;
                let secs = (bin as f64 + 0.5) / num_peaks as f64 * duration_secs;
                let frame = (secs * fps) as FrameIdx;
                if !clip.contains(frame) {
                    continue;
                }
                let source_secs = clip.media_secs_at(frame, fps);
                let source_bin = ((source_secs / source_wf.audio_duration_secs
                    * source_wf.peaks.len() as f64) as usize)
                    .min(source_wf.peaks.len() - 1);
                let source_frame = (source_secs * source.meta.fps.as_f64()) as FrameIdx;
                let gain =
                    vv_audio::mixer::db_to_linear(clip.effects.gain_db.value_at(source_frame));
                *peak = peak.max(source_wf.peaks[source_bin] * gain);
            }
        }
    }
    Some((
        vv_media::Waveform {
            peaks,
            audio_duration_secs: duration_secs,
        },
        complete,
    ))
}

#[cfg(test)]
fn track_end(project: &vv_core::Project, timeline_id: TimelineId, track_index: usize) -> FrameIdx {
    project.timelines[timeline_id]
        .tracks
        .get(track_index)
        .and_then(|t| t.clips.iter().map(|c| c.timeline_end()).max())
        .unwrap_or(0)
}

/// Duration of a media for the panel column: MM:SS, with the hours only
/// when there are any.
pub(crate) fn format_duration(duration_frames: vv_core::FrameIdx, fps: f64) -> String {
    let secs = (duration_frames.max(0) as f64 / fps.max(1e-9)).round() as u64;
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Color,
}

/// Hand-drawn icon toggle, filled with the accent when on.
fn icon_toggle(ui: &mut egui::Ui, on: &mut bool, icon: ToolbarIcon) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let visuals = ui.style().interact_selectable(&response, *on);
    let painter = ui.painter();
    if *on || response.hovered() {
        painter.rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    paint_toolbar_icon(painter, icon, rect.center(), visuals.fg_stroke.color);
    response
}

fn paint_toolbar_icon(
    painter: &egui::Painter,
    icon: ToolbarIcon,
    c: egui::Pos2,
    color: egui::Color32,
) {
    let stroke = egui::Stroke::new(1.3, color);
    match icon {
        // A color wheel with its puck off center.
        ToolbarIcon::Color => {
            painter.circle_stroke(c, 6.0, stroke);
            painter.line_segment([c - egui::vec2(6.0, 0.0), c + egui::vec2(6.0, 0.0)], stroke);
            painter.line_segment([c - egui::vec2(0.0, 6.0), c + egui::vec2(0.0, 6.0)], stroke);
            painter.circle_filled(c + egui::vec2(2.6, -2.6), 2.0, color);
        }
    }
}

/// A dot and a short text in `color`, for a state of the editor.
fn status_badge(ui: &mut egui::Ui, text: String, color: egui::Color32) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text, egui::TextStyle::Button.resolve(ui.style()), color);
    let size = egui::vec2(18.0 + galley.size().x, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    painter.circle_filled(egui::pos2(rect.left() + 6.0, rect.center().y), 3.5, color);
    painter.galley(
        egui::pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

/// Shown while an MCP client is connected: an accent dot and "MCP".
fn agent_indicator(ui: &mut egui::Ui, last_tool: Option<&str>) {
    let response = status_badge(ui, "MCP".to_owned(), theme::ACCENT);
    response.on_hover_ui(|ui| {
        ui.label(t!("toolbar.mcp_connected"));
        if let Some(tool) = last_tool {
            ui.label(t!("toolbar.mcp_last_tool", tool = tool));
        }
    });
}

/// Box with four corner handles and the pivot at the center.
fn transform_overlay_toggle(ui: &mut egui::Ui, enabled: &mut bool) -> egui::Response {
    let size = egui::vec2(26.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *enabled = !*enabled;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, *enabled);
        let painter = ui.painter();
        painter.rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
        let color = visuals.fg_stroke.color;
        let frame = egui::Rect::from_center_size(rect.center(), egui::vec2(14.0, 11.0));
        painter.rect_stroke(
            frame,
            0.0,
            egui::Stroke::new(1.2, color),
            egui::StrokeKind::Middle,
        );
        for corner in [
            frame.left_top(),
            frame.right_top(),
            frame.left_bottom(),
            frame.right_bottom(),
        ] {
            painter.rect_filled(
                egui::Rect::from_center_size(corner, egui::vec2(4.0, 4.0)),
                0.0,
                color,
            );
        }
        painter.circle_stroke(frame.center(), 2.0, egui::Stroke::new(1.2, color));
    }
    response
}

/// Film strip and waveform icons at the bottom of the viewer: dragging them brings
/// only the video or only the audio onto the timeline.
fn show_stream_drag_handles(
    ui: &egui::Ui,
    viewer: egui::Rect,
) -> [(egui::Response, timeline_ui::DragStreams); 2] {
    const SIZE: egui::Vec2 = egui::vec2(30.0, 26.0);
    const GAP: f32 = 6.0;
    let center = egui::pos2(viewer.center().x, viewer.bottom() - 12.0 - SIZE.y / 2.0);
    let offset = egui::vec2((SIZE.x + GAP) / 2.0, 0.0);
    let video_rect = egui::Rect::from_center_size(center - offset, SIZE);
    let audio_rect = egui::Rect::from_center_size(center + offset, SIZE);

    let video = ui
        .interact(
            video_rect,
            ui.id().with("viewer_drag_video_only"),
            egui::Sense::drag(),
        )
        .on_hover_text(t!("viewer.drag_video_only"));
    let audio = ui
        .interact(
            audio_rect,
            ui.id().with("viewer_drag_audio_only"),
            egui::Sense::drag(),
        )
        .on_hover_text(t!("viewer.drag_audio_only"));

    let visible = ui.rect_contains_pointer(viewer) || video.dragged() || audio.dragged();
    if visible {
        let painter = ui.painter();
        for resp in [&video, &audio] {
            let alpha = if resp.hovered() || resp.dragged() {
                220
            } else {
                150
            };
            painter.rect_filled(resp.rect, 4.0, egui::Color32::from_black_alpha(alpha));
        }
        let color = egui::Color32::from_gray(230);

        let film = egui::Rect::from_center_size(video_rect.center(), egui::vec2(18.0, 14.0));
        painter.rect_stroke(
            film,
            1.0,
            egui::Stroke::new(1.3, color),
            egui::StrokeKind::Middle,
        );
        for i in 0..4 {
            let x = film.left() + 3.0 + i as f32 * 4.0;
            for y in [film.top() + 2.0, film.bottom() - 2.0] {
                painter.rect_filled(
                    egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(2.0, 2.0)),
                    0.0,
                    color,
                );
            }
        }

        let heights = [4.0, 9.0, 14.0, 7.0, 12.0, 5.0, 8.0];
        let c = audio_rect.center();
        for (i, h) in heights.iter().enumerate() {
            let x = c.x + (i as f32 - 3.0) * 2.8;
            painter.line_segment(
                [egui::pos2(x, c.y - h / 2.0), egui::pos2(x, c.y + h / 2.0)],
                egui::Stroke::new(1.6, color),
            );
        }
    }

    [
        (video, timeline_ui::DragStreams::VideoOnly),
        (audio, timeline_ui::DragStreams::AudioOnly),
    ]
}

/// On Wayland winit sends `Started` for high-resolution scrolling but
/// almost never `Ended`: egui keeps Alt held and the zoom stays stuck.
/// Treated as `Move`, every event uses the current modifiers.
fn unstick_wheel_modifiers(raw_input: &mut egui::RawInput) {
    for event in &mut raw_input.events {
        if let egui::Event::MouseWheel { phase, .. } = event
            && *phase == egui::TouchPhase::Start
        {
            *phase = egui::TouchPhase::Move;
        }
    }
}

/// winit on macOS maps the ISO "<" key to `Backquote` and never emits
/// `IntlBackslash`; egui has no key for "<", so it falls back to that physical
/// code. The typed text tells the two apart; the release carries no text, so
/// `iso_key_down` rewrites it too, otherwise egui would see the key held.
#[cfg(target_os = "macos")]
fn fix_iso_key(raw_input: &mut egui::RawInput, iso_key_down: &mut bool) {
    for i in 0..raw_input.events.len() {
        let typed_angle = matches!(
            raw_input.events.get(i + 1),
            Some(egui::Event::Text(t)) if t.starts_with(['<', '>'])
        );
        if let egui::Event::Key { key, pressed, .. } = &mut raw_input.events[i]
            && *key == egui::Key::Backtick
        {
            if *pressed && typed_angle {
                *iso_key_down = true;
            }
            if *iso_key_down {
                *key = egui::Key::IntlBackslash;
                if !*pressed {
                    *iso_key_down = false;
                }
            }
        }
    }
}

/// In the top left corner of the frame: a keyframe stands in for a media the
/// decoder cannot keep up with.
fn paint_keyframes_badge(painter: &egui::Painter, frame: egui::Rect) {
    let galley = painter.layout_no_wrap(
        t!("viewer.keyframes_badge").into_owned(),
        egui::FontId::proportional(11.0),
        egui::Color32::WHITE,
    );
    let badge = egui::Rect::from_min_size(
        frame.left_top() + egui::vec2(8.0, 8.0),
        galley.size() + egui::vec2(10.0, 4.0),
    );
    painter.rect_filled(badge, 3.0, theme::ACCENT_FILL);
    painter.galley(
        badge.min + egui::vec2(5.0, 2.0),
        galley,
        egui::Color32::WHITE,
    );
}

/// One timeline frame every how many the buffer keeps: the speed while
/// playing fast, if the setting asks for it.
fn frame_stride(shown_frames_only: bool, playing: bool, speed: f64) -> FrameIdx {
    if shown_frames_only && playing {
        speed.round().max(1.0) as FrameIdx
    } else {
        1
    }
}

impl eframe::App for VenturiApp {
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        unstick_wheel_modifiers(raw_input);
        self.paste_key_watch.feed(raw_input);
        #[cfg(target_os = "macos")]
        fix_iso_key(raw_input, &mut self.iso_key_down);
        #[cfg(target_os = "linux")]
        if let Some(dnd) = &self.wayland_dnd {
            dnd.feed(raw_input, ctx.zoom_factor());
        }
        #[cfg(target_os = "linux")]
        if let Some(gestures) = &self.wayland_gestures {
            gestures.feed(raw_input);
        }
    }

    // The only hook eframe still calls while the window is minimized,
    // occluded or on another Space, when the open plugin editors need servicing.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_plugin_editors(ctx);
        self.poll_plugin_scan_hidden();
        if let Some(audio) = &self.timeline_audio {
            audio.service_plugin_restarts();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // With MCP on, the session lives in a slot shared with the agent's
        // host thread between frames (see `mcp_host`).
        self.mcp_checkout();
        self.frame_ui(ui, frame);
        self.mcp_checkin(&ui.ctx().clone());
    }

    /// Called by eframe on close and periodically (see
    /// `auto_save_interval`): the panel layout updated on every frame
    /// in `ui()` thus ends up on disk without writing it on every resize.
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.persist_settings();
    }

    // Quitting mid-scan is not a crash: keep the next launch from blaming the
    // bundle that is still loading.
    fn on_exit(&mut self) {
        self.abandon_plugin_scan();
        self.run_pending_relaunch();
    }
}

impl VenturiApp {
    fn undo_group_open(&self) -> bool {
        self.edit_drag_group.is_some() || self.timeline_state.holds_undo_group()
    }

    fn pump_plugin_editors(&mut self, ctx: &egui::Context) {
        let Some(borrowed) = self.mcp_borrow_session() else {
            return;
        };
        let group_open = self.undo_group_open();
        self.plugin_editors.update(
            ctx,
            &mut self.session.project,
            &mut self.session.history,
            self.timeline_id,
            self.timeline_audio.as_mut(),
            group_open,
        );
        if borrowed {
            self.mcp_return_session();
        }
    }

    // `frame_ui` does not run while the window is hidden; closing dropped
    // editors needs the real session.
    fn poll_plugin_scan_hidden(&mut self) {
        if self.plugin_scan_rx.is_none() {
            return;
        }
        let Some(borrowed) = self.mcp_borrow_session() else {
            return;
        };
        self.poll_plugin_scan();
        if borrowed {
            self.mcp_return_session();
        }
    }

    fn frame_ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_close_request(&ui.ctx().clone());
        self.poll_pending_dialog(&ui.ctx().clone());
        self.poll_project_loading(&ui.ctx().clone());
        self.poll_ai_sequences(ui.ctx());
        self.show_update_dialog(ui.ctx());
        if self.screen == Screen::Home {
            self.auto_cut.modal = None;
            self.home_ui(ui);
            return;
        }
        self.poll_dropped_files(&ui.ctx().clone());
        self.poll_clipboard_image();
        self.poll_background_removal(&ui.ctx().clone());
        self.poll_plugin_scan();
        self.follow_hw_decode_fallback();
        // What the session applies would land inside the open group.
        if self.edit_drag_group.is_none() && !self.timeline_state.holds_undo_group() {
            self.poll_session(&ui.ctx().clone());
            self.poll_stabilize_job(&ui.ctx().clone());
            self.poll_audio_sync(&ui.ctx().clone());
            self.poll_tracking(&ui.ctx().clone());
        }
        let screenshots: Vec<std::sync::Arc<egui::ColorImage>> = ui.ctx().input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
                .collect()
        });
        for image in screenshots {
            self.mcp_screenshot(&image);
        }
        if self.mcp_awaits_screenshot() {
            ui.ctx().request_repaint();
        }
        // Here and not in the pool: the viewer needs it with the pool closed too.
        self.media_pool_state.refresh_offline(&self.session.project);
        self.poll_thumbnails(&ui.ctx().clone());
        if self
            .thumbnail_worker
            .as_ref()
            .is_some_and(|w| w.has_pending())
        {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        // The jump is applied by the timeline scroll sync of this frame; the
        // command that set it may have run after the timeline was drawn.
        if self.timeline_state.reveal_playhead {
            ui.ctx().request_repaint();
        }
        if let Some(audio) = &mut self.timeline_audio {
            audio.tick();
            self.playback_speed = audio.speed();
            if audio.is_scrub_snippet_active() {
                ui.ctx().request_repaint();
            }
        }
        self.sync_timeline_audio();
        if let Some(audio) = &self.timeline_audio
            && let Some(at) = audio.next_retry()
        {
            ui.ctx()
                .request_repaint_after(at.saturating_duration_since(std::time::Instant::now()));
        }
        self.sync_window_title(ui.ctx());

        if self.auto_cut.modal.is_some() {
            self.tick_autosave(&ui.ctx().clone());
            self.show_auto_cut(ui.ctx());
            return;
        }
        self.handle_shortcuts(ui);
        self.tick_autosave(&ui.ctx().clone());

        self.show_top_bar(ui);

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.ai_activity() > 0 {
                    ui.spinner();
                    ui.label(t!("ai_sequence.active_tasks", count = self.ai_activity()));
                }
                if let Some(err) = &self.project_error {
                    ui.separator();
                    ui.colored_label(theme::ERROR, err);
                }
                self.show_download_indicator(ui);
                if let Some((done, total)) = self.import_progress() {
                    ui.separator();
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                            .desired_width(180.0)
                            .text(t!("project.import_progress", done = done, total = total)),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(mcp) = self.mcp.as_ref().filter(|mcp| mcp.clients() > 0) {
                        agent_indicator(ui, mcp.last_tool());
                    }
                });
            });
        });

        self.show_export_dialog(ui);
        self.show_export_modal(ui.ctx());
        self.poll_dub(ui.ctx());
        self.show_dub(ui.ctx());
        self.show_cover_picker(ui.ctx());
        self.poll_subtitles(ui.ctx());
        self.poll_ai_clips(ui.ctx());
        self.show_recovery_notice(ui.ctx());
        self.poll_downloads(ui.ctx());
        self.show_download_dialog(ui.ctx());
        self.show_toast(ui.ctx());
        self.show_beat_window(ui.ctx());
        self.show_settings_dialog(ui.ctx());
        self.show_about_dialog(ui.ctx());
        self.show_project_settings(ui.ctx());
        self.show_export_progress(ui);
        self.show_import_warnings(ui);
        self.show_otio_import_progress(ui);
        self.show_otio_merge_dialog(ui);
        self.show_relink_progress(ui);
        self.show_relink_message(ui);
        self.show_forced_relink_dialog(ui.ctx());
        self.show_unsaved_changes_dialog(ui);
        self.show_new_timeline_dialog(ui.ctx());
        self.show_paste_image_dialog(ui.ctx());
        self.show_paste_image_needs_save(ui.ctx());
        if std::mem::take(&mut self.timeline_state.paste_attributes_requested) {
            self.open_paste_attributes_dialog();
        }
        self.show_paste_attributes_dialog(ui.ctx());
        if std::mem::take(&mut self.timeline_state.edit_cover) {
            self.open_cover_picker();
        }
        if let Some(clips) = self.timeline_state.save_preset_requested.take() {
            self.open_save_preset_dialog(&clips);
        }
        self.show_preset_dialog(ui.ctx());
        if let Some((preset, targets)) = self.timeline_state.preset_drop.take() {
            self.apply_preset_by_id(&preset, &targets);
        }
        self.poll_library();
        self.poll_library_copies();
        if let Some(targets) = self.timeline_state.speed_dialog_requested.take() {
            self.open_speed_dialog(targets);
        }
        if let Some(media) = self.timeline_state.reveal_in_pool_requested.take() {
            self.reveal_in_media_pool(media);
        }
        self.show_speed_dialog(ui.ctx());
        if let Some(targets) = self.timeline_state.silence_dialog_requested.take() {
            self.open_silence_dialog(ui.ctx(), targets);
        }
        self.show_silence_dialog(ui.ctx());

        let (video_targets, audio_targets) = self.selection_targets();
        let panel_targets = self.with_linked_subtitles(&video_targets);

        self.sync_layout_density(ui.ctx());
        self.show_layout_proposal(ui);
        let rects = egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG_WINDOW))
            .show(ui, |ui| {
                if self.vertical_layout() {
                    self.vertical_layout_ui(ui)
                } else if self.workspace.has_docked_blocks() {
                    self.workspace.layout_ui(ui)
                } else {
                    ui.centered_and_justified(|ui| ui.weak(t!("workspace.nothing_docked")));
                    workspace::BlockRects::new()
                }
            })
            .inner;
        self.block_rects = rects.keys().copied().collect();

        let mut preview_action = None;
        self.show_block(ui, &rects, Block::Library, |app, ui| {
            app.show_library_block(ui, &mut preview_action)
        });
        if !self.block_shown(Block::Library) {
            self.media_pool_state.focused = false;
        }

        let selected_before_timeline_ui = self.timeline_state.selected.clone();
        let playhead_before_timeline_ui = self.timeline_state.playhead;
        let (media_drop, dropped_on_empty_timeline) = self
            .show_block(ui, &rects, Block::Timeline, Self::show_timeline_block)
            .unwrap_or_default();
        if let Some(drag) = dropped_on_empty_timeline {
            self.add_drop_to_timeline_at(&drag, 0, timeline_ui::MediaDropTarget::Default);
        }
        if let Some((drag, start, target)) = media_drop {
            self.add_drop_to_timeline_at(&drag, start, target);
        }

        // A user scrub must be followed during playback too, unlike
        // the playhead moved by `drive_playback`.
        let user_scrubbed_playhead = self.timeline_state.playhead != playhead_before_timeline_ui;
        let selection_changed = self.timeline_state.selected != selected_before_timeline_ui;
        if user_scrubbed_playhead {
            self.stop_voiceover();
        }
        if user_scrubbed_playhead && !selection_changed {
            self.sync_selection_to_playhead();
        }

        // Interacting with the timeline (selecting a clip or moving the
        // playhead) takes control of the viewer back from the "raw"
        // media pool preview, if active.
        if self.browsing.is_some()
            && (self.timeline_state.selected != selected_before_timeline_ui
                || user_scrubbed_playhead)
        {
            self.stop_browsing();
        }

        if self.timeline_state.scrubbing && self.browsing.is_none() {
            self.hold_playback_while_scrubbing(true);
        }
        if self.browsing.is_none() {
            self.ensure_active_clip_matches_playhead(user_scrubbed_playhead);
            if user_scrubbed_playhead {
                self.play_scrub_audio();
            }
            self.drive_playback();
        }
        self.drive_browse_playback();
        self.drive_take();
        // The timeline buffer stays warm during the pool preview too.
        self.sync_render_ahead();
        // The check above ran before the sync: a setting changed by it
        // would otherwise wait for an unrelated repaint.
        if self
            .render_ahead
            .as_ref()
            .is_some_and(|r| !r.is_caught_up())
        {
            ui.ctx().request_repaint();
        }
        self.sync_root_timeline_media();

        let (mut pending_effects, mut pending_playhead) = self
            .show_block(ui, &rects, Block::Properties, |app, ui| {
                app.show_properties_panel(ui, &panel_targets, &audio_targets)
            })
            .unwrap_or_default();

        pending_effects.extend(std::mem::take(&mut self.library_ribbon.editor_commands));
        pending_playhead = self
            .library_ribbon
            .editor_playhead
            .take()
            .or(pending_playhead);

        if let Some(browsed) = preview_action {
            self.preview(&browsed);
            // Pool preview: no active clip, and the playhead must not
            // take it away on the next frame.
            self.active_clip = None;
            self.browsing = Some(browsed);
            if std::mem::take(&mut self.library_ribbon.autoplay) {
                self.toggle_browse_playback();
            }
        }
        if let Some(frame) = pending_playhead {
            self.stop_voiceover();
            self.timeline_state.playhead = frame.max(0);
        }

        // A change from the panel can touch several clips (multiple
        // selection): a single composite command, so the undo brings them
        // all back together.
        // An open dropdown is applying the preview of its entries:
        // its changes go into a single undo step, like a drag.
        let holding = ui.input(|i| i.pointer.any_down()) || preview_combo_open(ui.ctx());
        self.apply_effect_changes(pending_effects, holding);
        if pending_playhead.is_some() {
            self.ensure_active_clip_matches_playhead(true);
            self.timeline_state.reveal_playhead = true;
            ui.ctx().request_repaint();
        }
        if let Some(request) = self.mixer_panel.editor_requested.take() {
            let current = self.timeline_id;
            self.timeline_audio();
            if let Some(audio) = &mut self.timeline_audio {
                self.plugin_editors
                    .open(ui.ctx(), &self.session.project, request, current, audio);
                ui.ctx().request_repaint();
            }
        }

        let mut overlay_effects: Vec<BoxedCommand> = Vec::new();
        let mut transport_action = self
            .show_block(ui, &rects, Block::Player, |app, ui| {
                app.show_player_block(ui, &panel_targets, &mut overlay_effects)
            })
            .unwrap_or_default();

        if self.viewer_fullscreen {
            transport_action = self.show_fullscreen_viewer(ui.ctx());
        }
        if transport_action.toggle_fullscreen {
            self.viewer_fullscreen = !self.viewer_fullscreen;
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.viewer_fullscreen));
        }
        if transport_action.fit_view {
            self.viewer_zoom.fit();
        }
        if let Some(quality) = transport_action.quality {
            self.settings.preview_resolution = quality;
            self.apply_preview_resolution();
        }
        if let Some(ratio) = transport_action.ratio {
            self.set_project_ratio(ratio);
        }
        if let Some(choice) = transport_action.safe_zones {
            self.settings.shorts.safe_zones = choice.is_some();
            if let Some(id) = choice {
                self.settings.shorts.safe_zone_platform = id;
                self.safe_zone_platforms = safe_zones::load();
            }
        }

        let pointer_down = ui.input(|i| i.pointer.any_down());
        self.apply_effect_changes(overlay_effects, pointer_down);
        self.update_scopes(ui.ctx());

        if transport_action.toggle_play {
            self.toggle_playback();
        }
        let scrubbing =
            std::mem::take(&mut self.timeline_state.scrubbing) || transport_action.scrubbing;
        if self.browsing.is_none() {
            self.hold_playback_while_scrubbing(scrubbing);
        }
        if let Some((mark_in, mark_out)) = transport_action.marks
            && let Some(target) = self.mark_target()
        {
            target.marks.mark_in = (mark_in > 0).then_some(mark_in);
            target.marks.mark_out = (mark_out < target.total).then_some(mark_out);
        }
        if let Some(frame) = transport_action.seek {
            self.stop_voiceover();
            if self.browsing.is_some() {
                self.seek_browse(frame);
            } else {
                self.timeline_state.playhead = frame;
                self.ensure_active_clip_matches_playhead(true);
                self.sync_selection_to_playhead();
                self.play_scrub_audio();
            }
            ui.ctx().request_repaint();
        }

        self.show_voiceover(ui.ctx(), rects.get(&Block::Timeline).copied());
        self.show_record_needs_save(ui.ctx());

        if self.is_timeline_playing() {
            ui.ctx().request_repaint();
        }

        // Repaint during the drags, otherwise they are not smooth with the player stopped.
        if ui
            .ctx()
            .input(|i| i.pointer.any_down() || i.pointer.any_released())
        {
            ui.ctx().request_repaint();
        }
    }

    /// Targets of the panels: the selected clips by track kind, in order
    /// (track, start). The frame of each is the playhead in its own source
    /// space; the first gives the shown values, the changes go to all of them.
    pub(crate) fn selection_targets(&self) -> (Vec<PanelTarget>, Vec<PanelTarget>) {
        let mut video_targets: Vec<PanelTarget> = Vec::new();
        let mut audio_targets: Vec<PanelTarget> = Vec::new();
        if let Some(timeline_id) = self.timeline_id {
            let tl = &self.session.project.timelines[timeline_id];
            for &(track_index, clip_id) in &self.timeline_state.selected {
                let Some(track) = tl.tracks.get(track_index) else {
                    continue;
                };
                let Some(clip) = track.clip(clip_id) else {
                    continue;
                };
                let target =
                    panel_target(timeline_id, track_index, clip, self.timeline_state.playhead);
                match track.kind {
                    vv_core::TrackKind::Video => video_targets.push(target),
                    vv_core::TrackKind::Audio => audio_targets.push(target),
                }
            }
        }
        for targets in [&mut video_targets, &mut audio_targets] {
            targets.sort_by_key(|t| (t.track_index, t.timeline_start));
        }
        (video_targets, audio_targets)
    }

    /// Draws `block` where it lives: in its tile of the main window, in its
    /// own window, or nowhere if it is closed.
    fn show_block<R>(
        &mut self,
        ui: &mut egui::Ui,
        rects: &workspace::BlockRects,
        block: Block,
        content: impl FnOnce(&mut Self, &mut egui::Ui) -> R,
    ) -> Option<R> {
        if let Some(&rect) = rects.get(&block) {
            ui.painter()
                .rect_filled(rect, theme::panel_radius(), theme::BG_PANEL);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("block", block))
                    .max_rect(rect.shrink(theme::SPACE_1)),
            );
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            return Some(content(self, &mut child));
        }
        // The vertical layout has no separate windows: they come back with
        // the desktop one.
        if self.vertical_layout() {
            return None;
        }
        let builder = self.workspace.viewport_builder(block)?;
        let ctx = ui.ctx().clone();
        let mut content = Some(content);
        let mut redock = false;
        let result = ctx.show_viewport_immediate(block.viewport_id(), builder, |ui, _| {
            self.workspace.track_window(block, ui);
            redock |= ui.input(|i| i.viewport().close_requested());
            // Keys typed here reach only this window's input.
            if ui.input(|i| i.focused) {
                self.handle_shortcuts(ui);
            }
            egui::Panel::top(egui::Id::new(("block_bar", block))).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(block.title());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        redock |= ui.button(t!("workspace.redock")).clicked();
                    });
                });
            });
            egui::CentralPanel::default()
                .show(ui, |ui| content.take().map(|content| content(self, ui)))
                .inner
        });
        if redock {
            self.workspace.redock(block);
            ui.ctx().request_repaint();
        }
        result
    }

    fn show_library_block(&mut self, ui: &mut egui::Ui, preview_action: &mut Option<Browsed>) {
        // A block with both sections off shows the pool rather than nothing.
        let pool = ui.scope(|ui| self.show_ribbon(ui, preview_action)).response;
        // Whoever got the last click decides who gets Del/Backspace.
        if let Some(pos) = ui.ctx().input(|i| {
            i.pointer
                .any_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        }) {
            self.media_pool_state.focused = pool.rect.contains(pos);
        }
        // Highlights the pool while a file is dragged from the file manager.
        if ui.ctx().input(|i| !i.raw.hovered_files.is_empty()) {
            ui.ctx()
                .layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("media_pool_drop_highlight"),
                ))
                .rect_stroke(
                    pool.rect,
                    4.0,
                    egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                    egui::StrokeKind::Inside,
                );
        }
    }

    /// The media dropped on the timeline, and a drop made when there was no
    /// timeline yet.
    fn show_timeline_block(
        &mut self,
        ui: &mut egui::Ui,
    ) -> (Option<MediaDrop>, Option<timeline_ui::TimelineDrag>) {
        let mut media_drop = None;
        let mut dropped_on_empty_timeline = None;
        if self.audiometer_enabled {
            // Narrow band on the right, carved out *before*
            // showing the timeline: it steals only this fixed
            // width from it, it does not compress it proportionally.
            egui::Panel::right("audiometer")
                .default_size(40.0)
                .resizable(false)
                .show(ui, |ui| {
                    self.draw_audiometer(ui);
                });
        }
        let can_undo = self.session.history.position() > 0;
        let can_redo = self.session.history.labels().count() > self.session.history.position();
        let can_voiceover = self
            .timeline_id
            .is_some_and(|id| self.session.project.timelines[id].total_frames() > 0);
        let can_auto_cut = self.take.is_none()
            && self.timeline_id.is_some_and(|id| {
                let timeline = &self.session.project.timelines[id];
                self.timeline_state.selected.iter().any(|&(t, c)| {
                    timeline
                        .clip(t, c)
                        .is_some_and(|clip| matches!(clip.source, vv_core::ClipSource::Media(_)))
                })
            });
        egui::Panel::top("timeline_bar")
            .resizable(false)
            .show(ui, |ui| {
                timeline_bar::show_bar(
                    ui,
                    &mut self.timeline_state,
                    &mut self.snapping_enabled,
                    &self.settings.keymap,
                    can_undo,
                    can_redo,
                    can_auto_cut,
                    can_voiceover,
                );
            });
        self.timeline_state.thumbnails.clear();
        for (hash, texture) in &self.thumbnails {
            if let Some(texture) = texture {
                self.timeline_state
                    .thumbnails
                    .insert(*hash, (texture.id(), texture.size_vec2()));
            }
        }
        let exit_requested = std::mem::take(&mut self.exit_compound_requested);
        if let Some(timeline_id) = self.timeline_id {
            if !self.timeline_stack.is_empty() {
                let mut jump_to = exit_requested.then(|| self.timeline_stack.len() - 1);
                ui.horizontal(|ui| {
                    for i in 0..self.timeline_stack.len() {
                        let name = self.timeline_display_name(self.timeline_stack[i]);
                        if ui.link(name).clicked() {
                            jump_to = Some(i);
                        }
                        ui.label(">");
                    }
                    ui.label(self.timeline_display_name(timeline_id));
                });
                if let Some(index) = jump_to {
                    self.exit_to_timeline_stack_index(index);
                }
            }
            // A breadcrumb click switched timeline: drawing the one
            // left would spend the playhead reveal on it.
            let timeline_id = self.timeline_id.unwrap_or(timeline_id);
            let labels: HashMap<MediaId, String> = self
                .session
                .project
                .media_pool
                .iter()
                .map(|(id, item)| (id, file_label(&item.path)))
                .collect();
            let buffered_ranges = self.buffered_timeline_ranges();
            let keyframe_ranges = self.keyframe_timeline_ranges();
            let proxy_ranges = self.proxy_timeline_ranges();
            // The buffer advances on another thread: without a repaint the
            // "buffered" strip would not update.
            if self
                .render_ahead
                .as_ref()
                .is_some_and(|r| !r.is_caught_up())
            {
                ui.ctx().request_repaint();
            }
            // Audio peaks already in memory (loaded from the cache file
            // the first time they are needed, see `ensure_waveforms_loaded`):
            // the timeline draws them as waveforms on the audio clips.
            self.ensure_waveforms_loaded();
            let is_playing = self.is_timeline_playing();
            let audio_failures = self
                .timeline_audio
                .as_ref()
                .map(|audio| audio.clip_failures(&self.session.project, timeline_id))
                .unwrap_or_default();
            self.timeline_state.ai_placeholders = self.ai_placeholders();
            let (drop, enter_compound) = timeline_ui::show_timeline(
                ui,
                &mut self.session.project,
                &mut self.session.history,
                timeline_id,
                &|id| labels.get(&id).cloned().unwrap_or_default(),
                &audio_failures,
                &mut self.timeline_state,
                self.snapping_enabled,
                self.settings.kinetic_scroll,
                &buffered_ranges,
                &keyframe_ranges,
                &proxy_ranges,
                &self.waveform_cache,
                is_playing,
            );
            media_drop = drop;
            self.apply_ai_placeholder_action();
            if let Some((nested_id, clip, at)) = enter_compound {
                self.enter_compound_timeline(nested_id, at, Some(clip));
            }
        } else {
            dropped_on_empty_timeline =
                timeline_bar::show_empty(ui, timeline_ui::TRACK_HEADER_WIDTH);
        }
        let ctx = ui.ctx().clone();
        self.run_timeline_commands(&ctx);
        (media_drop, dropped_on_empty_timeline)
    }

    fn show_player_block(
        &mut self,
        ui: &mut egui::Ui,
        video_targets: &[PanelTarget],
        overlay_effects: &mut Vec<BoxedCommand>,
    ) -> transport::TransportResponse {
        let mut transport_action = transport::TransportResponse::default();
        let mut viewer_rect = None;
        // The whole player box, bars included: the handles near the
        // edge of the frame must stay grabbable outside too.
        let mut viewer_area = None;
        egui::Panel::bottom("view_toggles")
            .default_size(28.0)
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    transform_overlay_toggle(ui, &mut self.show_transform_overlay)
                        .on_hover_text(t!("toolbar.transform_overlay"));
                });
            });
        let (total, playhead, marks, playing) = self.transport_state();
        let fps = self.transport_fps();
        let options = self.player_options();
        egui::Panel::bottom("transport")
            .resizable(false)
            .show(ui, |ui| {
                transport_action =
                    transport::show_transport(ui, total, playhead, marks, playing, fps, options);
            });
        egui::Panel::top("viewer_zoom_bar")
            .resizable(false)
            .show(ui, |ui| self.show_viewer_zoom_bar(ui));
        // Preview Axis: the player shows the frame under the axis while the
        // playhead stays; restored before anything else reads it.
        let real_playhead = self.timeline_state.playhead;
        if let Some(frame) = self.preview_axis_frame() {
            self.timeline_state.playhead = frame;
            if self.hover_jumped != Some(frame) {
                if let Some(render_ahead) = &self.render_ahead {
                    render_ahead.jump_to(frame);
                }
                self.hover_jumped = Some(frame);
            }
        } else if self.hover_jumped.take().is_some()
            && let Some(render_ahead) = &self.render_ahead
        {
            render_ahead.jump_to(real_playhead);
        }
        let slip_split = self.slip_split_layers();
        if slip_split.is_some() {
            // The buffers of the two ends fill on their own threads.
            ui.ctx().request_repaint();
        }
        let media_offline = slip_split.is_none() && self.active_clip_media_offline();
        // Browsing a "raw" media from the media pool there is no
        // timeline to compose: a single layer, its frame as it is.
        let layers = if let Some(split) = slip_split {
            self.viewer_keyframe = false;
            split
        } else if media_offline || self.browsing.is_some() {
            self.viewer_keyframe = false;
            None
        } else {
            let layers = self.timeline_video_layers(None);
            let served_keyframe = self
                .render_ahead
                .as_mut()
                .is_some_and(|r| r.take_served_keyframe());
            // No layers: the frame already shown stays, and its badge.
            if layers.is_some() {
                self.viewer_keyframe = served_keyframe;
            }
            layers
        };

        if self.browsing.is_none()
            && let Some(id) = self.timeline_id
        {
            let timeline = &self.session.project.timelines[id];
            let output = if let Some(layers) = &layers
                && !layers.is_empty()
            {
                self.viewer_output(layers)
            } else {
                vv_render::OutputFrame::exact(timeline.resolution.0, timeline.resolution.1)
                    .reduced(self.settings.preview_resolution.divisor())
            };
            text_prewarm::prepare(timeline, self.timeline_state.playhead, output);
        }

        if media_offline {
            self.last_viewer_frame_kind = Some(ViewerFrameKind::Offline);
        } else if self.browsing.is_some() {
            if self.preview_meta.as_ref().is_some_and(|m| !m.has_video) {
                self.last_viewer_frame_kind = None;
            } else if let Some(frame) = self.browsing_video_frame() {
                let output = vv_render::OutputFrame::exact(frame.width, frame.height);
                let layer = frame_provider::OwnedLayer {
                    content: frame_provider::OwnedContent::Video {
                        source_size: (frame.width, frame.height),
                        frame,
                        matte: None,
                    },
                    transform: vv_core::Transform::default(),
                    opacity: 1.0,
                    filters: Vec::new(),
                    blend: vv_core::BlendMode::Normal,
                    masks: Vec::new(),
                    assets: Vec::new(),
                };
                self.show_composited(vec![layer], output);
            }
        } else if let Some(layers) = layers {
            let output = self.viewer_output(&layers);
            self.show_composited(layers, output);
        }

        self.timeline_state.playhead = real_playhead;
        match self.last_viewer_frame_kind {
            Some(ViewerFrameKind::Video) => {
                if let (Some(id), Some(tex_size)) = (self.video_texture_id, self.video_display_size)
                {
                    let (area, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    self.viewer_zoom.handle_input(ui, area, tex_size);
                    let ppp = ui.ctx().pixels_per_point();
                    let rect = self.viewer_zoom.frame_rect(area, tex_size, ppp);
                    ui.painter().with_clip_rect(area).image(
                        id,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    if self.viewer_keyframe {
                        paint_keyframes_badge(ui.painter(), rect.intersect(area));
                    }
                    self.viewer_geometry = Some((area, tex_size));
                    let scale = f64::from(self.viewer_zoom.scale(area, tex_size, ppp));
                    if scale != self.viewer_scale {
                        self.viewer_scale = scale;
                        ui.ctx().request_repaint();
                    }
                    viewer_area = Some(area);
                    viewer_rect = Some(rect);
                }
            }
            Some(ViewerFrameKind::Offline) => {
                ui.centered_and_justified(|ui| {
                    ui.colored_label(
                        theme::ERROR,
                        egui::RichText::new(t!("viewer.media_offline")).size(24.0),
                    );
                });
            }
            None => {
                if let Some(err) = &self.preview_error {
                    ui.colored_label(egui::Color32::RED, t!("viewer.player_error", error = err));
                } else if self.browsing_audio_only() {
                    let (rect, _) =
                        ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                    let total = self.browse_total_frames();
                    let keys = self.browsed_waveform_keys();
                    // Loaded here too: the timeline loads them before the
                    // double click that opened this media.
                    for &key in &keys {
                        self.load_waveform(key);
                    }
                    if keys.iter().any(|k| !self.waveform_cache.contains_key(k)) {
                        // The worker wakes no one when it is done.
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    let preview = viewer_waveform::AudioPreview {
                        waveforms: keys.iter().map(|k| self.waveform_cache.get(k)).collect(),
                        total_frames: total,
                        fps: self.browse_fps(),
                        playhead: self.browse_playhead,
                        marks: self.browse_marks.resolve(total),
                    };
                    viewer_waveform::paint(ui, rect, &preview);
                    // Dragged onto the timeline like a frame, clicked to seek.
                    viewer_rect = Some(rect);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if self.browsing.is_some() || self.active_clip.is_some() {
                            t!("viewer.decoding")
                        } else {
                            t!("viewer.empty_hint")
                        })
                    });
                }
            }
        }

        let Some(rect) = viewer_rect else {
            return transport_action;
        };
        let area = viewer_area.unwrap_or(rect);
        let ui = &mut self.viewer_zoom.tools_ui(ui, area);
        if viewer_area.is_some() {
            self.paint_safe_zones(ui, rect, area);
            self.show_viewer_overlay(ui, rect, area, video_targets, overlay_effects);
            self.show_reframe_faces(ui, rect, area);
        }

        if let Some(browsed) = self.browsing.clone() {
            let rect = rect.intersect(area);
            let (source_in, source_out) = self.browse_marks.resolve(self.browse_total_frames());
            let drag_id = ui.id().with("viewer_media_drag");
            let audio_only = self.browsing_audio_only();
            let sense = if audio_only {
                egui::Sense::click_and_drag()
            } else {
                egui::Sense::drag()
            };
            let hint = if audio_only {
                t!("viewer.waveform_hint")
            } else {
                t!("viewer.drag_in_out")
            };
            let resp = ui.interact(rect, drag_id, sense).on_hover_text(hint);
            if audio_only
                && resp.clicked()
                && let Some(pos) = resp.interact_pointer_pos()
            {
                let total = self.browse_total_frames();
                self.seek_browse(viewer_waveform::frame_at(rect, pos.x, total));
            }
            let mut drags = vec![(resp, timeline_ui::DragStreams::All)];
            if self
                .preview_meta
                .as_ref()
                .is_some_and(|m| m.has_video && m.has_audio)
            {
                drags.extend(show_stream_drag_handles(ui, rect));
            }
            for (resp, streams) in drags {
                // A library asset joins the project only once it is dragged.
                let media_id = match &browsed {
                    Browsed::Media(id) => Some(*id),
                    Browsed::Asset(id) if resp.drag_started() => {
                        self.import_library_asset(id).first().copied()
                    }
                    Browsed::Asset(_) => None,
                };
                match media_id {
                    Some(media_id) => resp.dnd_set_drag_payload(timeline_ui::MediaDragSet::one(
                        timeline_ui::MediaDrag {
                            media_id,
                            source_in,
                            source_out,
                            streams,
                        },
                    )),
                    None if resp.hovered() => ui.ctx().set_cursor_icon(egui::CursorIcon::Grab),
                    None => {}
                }
                if resp.dragged()
                    && let Some(item) = self.browsed_item(&browsed)
                {
                    let name = file_label(&item.path);
                    let ghost = match streams {
                        timeline_ui::DragStreams::All => name,
                        timeline_ui::DragStreams::VideoOnly => {
                            t!("viewer.ghost_video_only", name = name).into_owned()
                        }
                        timeline_ui::DragStreams::AudioOnly => {
                            t!("viewer.ghost_audio_only", name = name).into_owned()
                        }
                    };
                    show_drag_ghost(ui, resp.id, &ghost);
                }
            }
        }
        transport_action
    }
}

fn app_icon() -> Option<egui::IconData> {
    const PNG: &[u8] = include_bytes!("../../../media/icons/png/tandem-icon-256.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(PNG));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

/// Desktop launches have no visible stderr: keep panics in `crash.log` next to the library.
fn install_crash_log() {
    let Some(path) = library::Library::default_root()
        .and_then(|root| root.parent().map(|dir| dir.join("crash.log")))
    else {
        return;
    };
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        use std::io::Write;
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(
                file,
                "--- unix time {seconds}, version {}, thread {}\n{info}\n{backtrace}",
                env!("CARGO_PKG_VERSION"),
                thread.name().unwrap_or("unnamed")
            );
        }
    }));
}

// The data paths are resolved from the XDG variables and HOME, which Windows
// does not set: they are mapped to the usual profile folders.
#[cfg(windows)]
fn windows_env_defaults() {
    let defaults = [
        ("HOME", "USERPROFILE"),
        ("XDG_CONFIG_HOME", "APPDATA"),
        ("XDG_DATA_HOME", "APPDATA"),
        ("XDG_CACHE_HOME", "LOCALAPPDATA"),
    ];
    for (name, source) in defaults {
        if std::env::var_os(name).is_none()
            && let Some(value) = std::env::var_os(source)
        {
            // SAFETY: called first in main, before any other thread starts.
            unsafe { std::env::set_var(name, value) };
        }
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    windows_env_defaults();
    env_logger::init();
    install_crash_log();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mcp") {
        run_mcp_command(&args[1..]);
    }
    let mcp_forced = args.iter().any(|a| a == "--mcp");
    args.retain(|a| a != "--mcp");
    // Files to open at startup, as if dropped on the window: this is also how
    // the file manager opens a `.tandem` project (`Exec=tandem %f`).
    let startup_paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    let settings_path = settings::Settings::default_path();
    let settings = settings_path
        .as_deref()
        .map(settings::Settings::load)
        .unwrap_or_default();
    subtitles::set_models_dir(settings.models_dir.clone());
    if settings.intel_experimental_decode {
        // SAFETY: no other thread started yet.
        unsafe { hw_decode::enable_intel_experimental_decode() };
    }
    std::thread::spawn(|| {
        vv_render::text::warm_up();
        fonts::register_bundled();
    });

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Tandem")
        // It must match the name of the .desktop, otherwise the Wayland
        // compositors do not associate the icon with the window.
        .with_app_id("tandem");
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport,
        wgpu_options: eframe::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetupCreateNew {
                device_descriptor: std::sync::Arc::new(|adapter| wgpu::DeviceDescriptor {
                    label: Some("egui wgpu device"),
                    required_limits: vv_render::device_limits(adapter),
                    ..Default::default()
                }),
                ..eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }
            .into(),
            ..Default::default()
        },
        ..Default::default()
    };

    eframe::run_native(
        "tandem",
        options,
        Box::new(move |cc| {
            // Timeline zoom with Ctrl+scroll. Shift+scroll is left free for the
            // vertical zoom of the tracks, so the horizontal scroll is Alt+scroll.
            cc.egui_ctx.options_mut(|o| {
                o.input_options.zoom_modifier = egui::Modifiers::CTRL;
                o.input_options.horizontal_scroll_modifier = egui::Modifiers::ALT;
            });
            theme::apply(&cc.egui_ctx);
            let mut app = VenturiApp::default();
            let egui_ctx = cc.egui_ctx.clone();
            let plugin_ctx = egui_ctx.clone();
            vv_plugin::set_restart_waker(std::sync::Arc::new(move || plugin_ctx.request_repaint()));
            app.session
                .set_waker(vv_session::Waker::new(move || egui_ctx.request_repaint()));
            app.settings_path = settings_path;
            if let Some(root) = library::Library::default_root() {
                vv_render::assets::set_user_dir(root.parent().map(|p| p.join("assets")));
                app.library = library::Library::open(root);
            }
            app.settings = settings;
            app.workspace = workspace::Workspace::from_value(app.settings.workspace.as_ref());
            app.settings.language.apply();
            // Seeded even when empty so nothing scans on the UI thread.
            vv_plugin::set_installed(app.settings.vst3_plugins.clone().unwrap_or_default());
            if app.settings.needs_plugin_scan() {
                app.start_plugin_scan(&cc.egui_ctx, false);
            }
            vv_media::hw::set_budget_bytes(app.settings.hw_decode_budget_bytes());
            hw_decode::start(app.settings.hw_decode);
            app.mcp_forced = mcp_forced;
            // Opened immediately: opening the audio stream blocks for hundreds of ms.
            app.timeline_audio = Some(TimelineAudio::new());
            if let Some(render_state) = cc.wgpu_render_state.clone() {
                app.compositor = vv_render::Compositor::new(
                    &render_state.adapter,
                    std::sync::Arc::new(render_state.device.clone()),
                    std::sync::Arc::new(render_state.queue.clone()),
                    app.session.project.precision,
                );
                cover::warm_up(&render_state, app.session.project.precision);
                app.egui_render_state = Some(render_state);
            }
            #[cfg(target_os = "linux")]
            {
                app.wayland_dnd = wayland_dnd::WaylandDnd::start(cc);
                app.wayland_gestures = wayland_gestures::WaylandGestures::start(cc);
            }
            let opens_project = startup_paths.iter().any(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(projects::PROJECT_EXTENSION))
            });
            // Media opened with the app need a project to land in.
            if !startup_paths.is_empty() && !opens_project {
                app.create_new_project();
            }
            app.open_paths(startup_paths);
            if app.session.path().is_none() {
                app.start_on_home_or_recover();
            }
            app.apply_mcp_setting(&cc.egui_ctx);
            if app.settings.check_updates && !cfg!(debug_assertions) {
                app.start_update_check(&cc.egui_ctx, false);
            }
            Ok(Box::new(app))
        }),
    )
}

/// `tandem mcp [project.tandem]`: an MCP server on stdin/stdout, no window.
/// `vv-app mcp --attach [--pid N]`: the same, served by a running editor.
/// Stdout carries the protocol: nothing else may print there.
fn run_mcp_command(args: &[String]) -> ! {
    if args.first().map(String::as_str) == Some("--attach") {
        let pid = match args.get(1..) {
            Some([flag, pid]) if flag == "--pid" => match pid.parse() {
                Ok(pid) => Some(pid),
                Err(_) => {
                    eprintln!("--pid needs a process id, got {pid}");
                    std::process::exit(2);
                }
            },
            Some([]) | None => None,
            Some(_) => {
                eprintln!("usage: vv-app mcp --attach [--pid N]");
                std::process::exit(2);
            }
        };
        match vv_mcp::bridge_stdio(pid) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    run_mcp_server(args.first().map(PathBuf::from))
}

/// Publishes a scan to `installed()` and caches it in the settings; returns
/// whether it matched the list already installed.
fn apply_scan_result(settings: &mut settings::Settings, list: Vec<vv_plugin::PluginInfo>) -> bool {
    let unchanged = vv_plugin::same_plugins(&list, &vv_plugin::installed());
    if !unchanged {
        vv_plugin::set_installed(list.clone());
    }
    settings.vst3_plugins = Some(list);
    unchanged
}

/// Headless processes never scan: they reuse the list the GUI cached, empty
/// until one GUI scan or rescan has run.
fn seed_installed_from_settings(path: Option<&Path>) {
    let plugins = path
        .map(settings::Settings::load)
        .and_then(|s| s.vst3_plugins)
        .unwrap_or_default();
    vv_plugin::set_installed(plugins);
}

fn run_mcp_server(project: Option<PathBuf>) -> ! {
    let protocol_out = match vv_mcp::protect_stdout() {
        Ok(out) => out,
        Err(e) => {
            eprintln!("cannot protect the MCP output stream: {e}");
            std::process::exit(1);
        }
    };
    seed_installed_from_settings(settings::Settings::default_path().as_deref());
    let mut session = vv_session::Session::default();
    if let Some(path) = project
        && let Err(e) = session.open(&path)
    {
        eprintln!("cannot open {}: {e}", path.display());
        std::process::exit(1);
    }
    match vv_mcp::serve_headless_stdio(session, protocol_out) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("MCP server: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
#[path = "tests/main.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/main_export.rs"]
mod export_tests;

/// The drawn part of a title, as `viewer_overlay::show` takes it: the union
/// of its layers (text, outline, shadow, background), from the same cached
/// raster the viewer shows.
impl VenturiApp {
    fn paint_safe_zones(&self, ui: &egui::Ui, rect: egui::Rect, area: egui::Rect) {
        if !self.settings.shorts.safe_zones
            || self.browsing.is_some()
            || !matches!(self.last_viewer_frame_kind, Some(ViewerFrameKind::Video))
        {
            return;
        }
        let Some(timeline_id) = self.timeline_id else {
            return;
        };
        let (platforms, file_error) = &self.safe_zone_platforms;
        let Some(platform) =
            safe_zones::platform(platforms, &self.settings.shorts.safe_zone_platform)
        else {
            return;
        };
        let tl = &self.session.project.timelines[timeline_id];
        let size = tl.resolution;
        let playhead = self.timeline_state.playhead;
        // Laying out every title costs a render: only while paused.
        let texts: Vec<[f32; 4]> = if self.is_timeline_playing() {
            Vec::new()
        } else {
            tl.tracks
                .iter()
                .filter(|t| t.kind == vv_core::TrackKind::Video && !t.muted)
                .flat_map(|t| &t.clips)
                .filter(|c| !c.disabled && c.contains(playhead))
                .filter_map(|c| {
                    let title = c.effects.title.as_ref()?;
                    let content = title_content_box(title, size)?;
                    let transform = c.effects.transform.value_at(c.source_frame_at(playhead));
                    let [l, r, b, t] =
                        viewer_overlay::content_frame_bounds(size, content, &transform);
                    let (w, h) = (size.0 as f32, size.1 as f32);
                    Some([l / w + 0.5, 0.5 - t / h, r / w + 0.5, 0.5 - b / h])
                })
                .collect()
        };
        let painter = ui.painter_at(rect.intersect(area));
        safe_zones::paint(&painter, rect, platform, &texts);
        if let Some(error) = file_error {
            painter.text(
                rect.left_bottom() + egui::vec2(4.0, -4.0),
                egui::Align2::LEFT_BOTTOM,
                t!("player.safe_zone_file_error", error = error),
                egui::FontId::proportional(theme::FONT_SMALL),
                theme::WARNING,
            );
        }
    }
}

fn title_content_box(title: &vv_core::TitleParams, timeline_size: (u32, u32)) -> Option<[f32; 4]> {
    let render = vv_render::text::render_title(title, timeline_size, timeline_size);
    let (half_w, half_h) = (timeline_size.0 as f32 / 2.0, timeline_size.1 as f32 / 2.0);
    render
        .masks()
        .iter()
        .filter(|(m, _)| m.mask.data.iter().any(|&c| c > 0))
        .map(|(m, _)| {
            let (x, y) = (m.x as f32, m.y as f32);
            let (w, h) = (m.mask.width as f32, m.mask.height as f32);
            [x - half_w, x + w - half_w, half_h - y - h, half_h - y]
        })
        .reduce(|a, b| {
            [
                a[0].min(b[0]),
                a[1].max(b[1]),
                a[2].min(b[2]),
                a[3].max(b[3]),
            ]
        })
}

impl VenturiApp {
    /// While a clip of `timeline` that removes its background still waits for
    /// its mattes, how far they are: an export now would show the background.
    /// A failed or stopped analysis does not hold the export back.
    pub(crate) fn cutout_pending(&self, timeline: TimelineId) -> Option<f32> {
        let project = &self.session.project;
        project.timelines[timeline]
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.effects.remove_background)
            .filter_map(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => project.media_pool.get(id),
                _ => None,
            })
            .filter(|item| {
                item.compound.is_none() && !vv_media::matte::mattes_complete(item.content_hash)
            })
            .filter_map(|item| match self.matte_worker.state(item.content_hash) {
                Some(matte_worker::MatteState::Failed(_)) => None,
                Some(matte_worker::MatteState::Running(progress))
                    if progress.stage() == vv_matting::Stage::Matting =>
                {
                    Some(progress.fraction())
                }
                _ => Some(0.0),
            })
            .reduce(f32::min)
    }

    /// What the clips of the timeline that remove their background need matted.
    fn matte_requests(&self) -> Vec<matte_worker::MatteRequest> {
        let Some(timeline_id) = self.timeline_id else {
            return Vec::new();
        };
        let project = &self.session.project;
        project.timelines[timeline_id]
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter(|c| c.effects.remove_background)
            .filter_map(|clip| match clip.source {
                vv_core::ClipSource::Media(id) => project.media_pool.get(id),
                _ => None,
            })
            .filter(|item| item.compound.is_none())
            .map(|item| matte_worker::MatteRequest {
                content_hash: item.content_hash,
                path: item.path.clone(),
                frames: item.meta.duration_frames,
                fps: item.meta.fps.as_f64(),
                image: item.meta.is_image(),
            })
            .collect()
    }

    /// Queues the mattes the clips that remove their background still lack,
    /// and redraws while they arrive.
    fn poll_background_removal(&mut self, ctx: &egui::Context) {
        for request in self.matte_requests() {
            self.matte_worker.request(request);
        }
        let progressed = self.matte_worker.poll();
        if self.matte_worker.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(if progressed {
                250
            } else {
                500
            }));
        }
    }
}
