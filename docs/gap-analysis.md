# Gap analysis — CapCut-style workflow (phase 0)

Source: `docs/spec-workflow.md` v2, compared against the code on `master`
(`ddb3263`). `handoff.md` does not exist in the repository; `ARCHITECTURE.md`,
`AGENTS.md` and `plans/` were used instead.

Status legend: **exists** · **partial** · **create** · **modify** (exists but
behaves differently from the spec). **⚙** = cannot be done without touching
the engine (`vv-media`, `vv-render`, `vv-audio`, the export/frame pipeline in
`vv-session`); see the questions at the end.

## 1. Spec → code

### 1. Project home

| Spec item | Status | Where |
|---|---|---|
| Card grid of `~/Videos/Projects` (configurable) | create | new `vv-app/src/home.rs`; path in `settings.rs` (today only `recent_projects`) |
| Card: thumbnail, name, size, duration, mtime | create | thumbnail from `thumbnail_worker.rs`; duration from the project's timeline |
| Search, sort/filter | create | `home.rs` |
| Card menu: open, rename, duplicate, delete, show in folder | create | `home.rs`; duplicate/rename must also move the project's media folder (see Q4) |
| New project with no dialog, `Progetto_NN` | modify | `project_io.rs` (today: New → save-as dialog, `new_timeline_dialog.rs` asks fps/resolution) |
| Empty state | create | `home.rs` |

### 2. Workspace layout

| Spec item | Status | Where |
|---|---|---|
| 4 blocks: library, player, properties, timeline | modify | `main.rs` (`Panel::left("left_column")`, `CentralPanel`, `properties_panel.rs` `Panel::right`, `Panel::bottom("timeline")`); default arrangement already close |
| Resizable dividers | exists | egui panels |
| Move blocks by dragging the header | create | needs a tiling layout (risk R1) |
| Detach a block to its own OS window and re-dock | create | egui multi-viewport (risk R1) |
| Layout saved/restored | partial | eframe `persistence` feature is on; the layout tree must be serialized |
| Top bar: Home, editable name, save indicator, Layout menu, Export | modify | `app_menu.rs` (classic menu bar today) |

### 3. Timeline

| Spec item | Status | Where |
|---|---|---|
| Single empty Main Track at start | modify | `vv-core` `Timeline` (no notion of a main track; new timelines get default tracks) |
| Auto-created tracks when dropping above/below | partial | drop onto new track exists in `timeline_ui.rs`; above = overlay, below = audio rule to add |
| Move clips between compatible tracks | exists | `MoveClips` |
| Fixed 120 px header: type icon, lock, eye, speaker | partial | `locked`, `muted` (= hidden on video), `solo` exist on `Track`; header layout to redo |
| Eye on a subtitle track hides it in the render | create | depends on subtitle tracks (phase 6) |
| Colours per clip type | partial | `ClipColor` exists, palette per kind to change |
| Filmstrip thumbnails on video clips | create | `timeline_ui.rs` + `thumbnail_worker.rs` (pool thumbnails only today) |
| Waveform on video clips with audio and on music | partial | waveform drawn on **audio** clips (`draw_clip_waveform`); video and audio are separate linked clips (Q3) |
| Trim handles `[` `]` with ↔ cursor, white border on selection | partial | trim exists; visuals to change |
| Fade dots on audio clips | partial | `fade_in`/`fade_out` exist on `Clip`; handles in the timeline to add |
| Click, Ctrl+click, rectangle, click on empty = project properties | partial | selection exists; "empty → project properties" to add |
| Mouse modes A / B (blade) / `[` / `]` | partial | select and slip tools exist; blade, select-left/right to create |
| Undo/redo, split at playhead | exists | `History`, `edit.rs` |
| `Q` / `W` delete left/right of playhead | create | `edit.rs` (`delete_ranges` is the base) |
| Delete selection | exists | `LiftDelete` |
| Main Track Magnet | modify | today ripple is **global on all tracks** by design (Q5) |
| Auto snapping | exists | `snapping_enabled` in `timeline_ui.rs`; toggle on `N` |
| Linkage (secondary clips follow the main clip) | create | `edit.rs` + `MoveClips` |
| Preview Axis (hover scrub without moving the playhead) | partial | audio scrub and `RenderAhead` seeking exist; a second "hover" position to add |
| Zoom slider, Ctrl+wheel, pinch | partial | Ctrl+/Ctrl- exist; slider and pinch to add |
| Split/Q/W on selection, else on all unlocked clips at the playhead | partial | split rule exists, extend to Q/W |
| Freeze Frame (3 s) | create | can be done without the engine: render the frame (`render_frame` exists for MCP) to a PNG and insert it as a still |
| Reverse | create ⚙ | needs backward decoding (`plans/REVERSE_SPEED.md`, not started) |
| Mirror | exists | `flip` in `Transform`; just a menu entry |
| Extract/Separate audio | modify | audio is already a separate linked clip; becomes "unlink + move" (Q3) |
| Compound clip / ungroup | partial | compound clips exist; "ungroup" (`Alt+Shift+G`) to create |
| Copy, cut, paste, duplicate, disable | exists | clipboard, `disabled` |

### 4. Library

| Spec item | Status | Where |
|---|---|---|
| Tabs Media/Audio/Text/Sticker/Effects/Transitions/Subtitles/Filters | modify | `media_pool_ui.rs`, `library_ui.rs` (pool + user library today) |
| Favourites per user, star on hover | partial | user library in `$XDG_DATA_HOME/venturi/library` (`library.rs`) is the natural store; favourites to add |
| `+` on hover inserts at the playhead | create | `library_ui.rs` / `media_pool_ui.rs` |
| Default durations (photo 5 s, sticker/text/effect 3 s) | partial | still and text clips exist; durations to align |
| Media: search, import button, drop, `Ctrl+I` | partial | import and DnD exist (`wayland_dnd.rs`); search to check/add |
| First media sets resolution/orientation/fps/ratio | create | `vv-core` insert path + `Timeline` settings; rotation metadata to verify in `MediaMeta` |
| Media empty state | create | `media_pool_ui.rs` |
| Audio: Music / Sound effects, click to preview | create | user library categories; preview through the pool preview path |
| Text: add default text | exists | `ClipSource::Text` |
| Text effects [placeholder], "Yours" templates | partial | attribute presets (`presets.rs`) are close to "save as template" |
| Stickers PNG/SVG/GIF/WebP | partial ⚙ | stills decode through FFmpeg; **SVG** and **animated GIF/WebP** need decoder support (Q1) |
| Effects/Filters on a clip and as a timeline item | partial | per-clip `filters` exist; "timeline item" = adjustment clip, which exists |
| Transitions only between adjacent clips (1-frame tolerance) | partial | `CrossTransition` exists; adjacency check and message to add |
| Transition default 0.5 s, clamped to available handles | partial | check against current clamping |
| Hover preview with the same shader on two bundled images | create | offscreen `Compositor` call from the UI (no engine change if done through the existing API) |
| Subtitles: generate, templates (word/line), manual [placeholder] | create | new; transcription separate from style (see R6) |

### 5. Player

| Spec item | Status | Where |
|---|---|---|
| Time / duration, play/pause | exists | `transport.rs` |
| Preview quality Full/½/¼ | partial | proxies at 640/960/1920 exist (`proxy.rs`); "¼ of the original" ≠ fixed widths (Q7) |
| Fullscreen `Ctrl+Shift+F` | exists | `FullscreenViewer` action; rebind |
| Ratio button = project ratio | partial | timeline resolution exists; `AspectMode` only in export |
| Gizmo: scale corners, crop sides, rotation knob, drag to move | exists | `viewer_overlay.rs` (has anchor handle too; corner scale to lock proportions) |
| Cyan centre guides with snap | create | `viewer_overlay.rs` |

### 6. Properties panel

| Spec item | Status | Where |
|---|---|---|
| Slider/drag/type, live, ↺ reset per control and section | exists | `properties_panel.rs` (reset per parameter and section already there) |
| Double-click slider knob = reset | create | `properties_panel.rs` |
| Basic: scale, position, rotate, blend + opacity | exists | 15 blend modes (spec lists 4) |
| Stabilize | done | camera motion measured by `vv-reframe::stabilize`, cancelled with generated keyframes (`vv_core::stabilize`) |
| Canvas (blur/colour background) | create | colour: solid clip under the clip; blur: duplicate layer + gaussian blur, done in the model without the engine (heavier) |
| Chroma key | create ⚙ | needs a shader |
| Auto Cutout | placeholder | none |
| Masks: None/Split/Filmstrip/Circle/Rectangle/Star/Heart + feather/invert | partial | rect/ellipse/bezier path with feather/invert exist; star/heart/split/filmstrip can be generated as paths/rects (no engine change) |
| Enhance | placeholder | none |
| Audio: volume −∞…+12 dB, fades | exists | `gain_db`, fades |
| Noise Reduction, Voice Changer | placeholder | none (VST3 hosting exists, but no bundled plugin) |
| Speed: 0.1×–100×, Keep Pitch | exists | `SetClipSpeed`, `pitch_correction` (`speed_dialog.rs`); range to check |
| Speed curve (ramp) | create ⚙ | keyframed `rate` reaches decode mapping and audio render (ARCHITECTURE "Not yet") |
| Animation In/Out/Combo, text Loop | create | can be generated as transform/opacity keyframes, no engine change |
| LUT `.cube`, HSL (8 colours), RGB curves, Sharpen, Vignette, Temp/Hue | create ⚙ | only colour wheels, saturation, exposure exist (`grade.rs`, `color.wgsl`) |
| Brightness/Contrast/Highlight/Shadow | partial | mappable onto existing wheels/exposure |
| Text: font, B/I/U, size, colour, align, background | exists | `TitleParams`, `TitleBackground` |
| Text: stroke, curved text | create ⚙ | `vv-render/src/text.rs` rasterizer |
| Text: shadow | exists | Drop Shadow filter |
| Save as template | partial | `presets.rs` |
| Transition: type and duration | exists | |
| Nothing selected: project name, ratio, fps, colour space | partial | `app_menu.rs` project settings window; colour space: export writes BT.709 only |

### 7. Keyframes

| Spec item | Status | Where |
|---|---|---|
| ◇ per property, ◀ ▶ navigation, update-or-create at playhead | exists | `properties_panel.rs`, `keyframe_editor.rs`, `UpsertKeyframe` |
| `Alt+Click` toggle | create | `properties_panel.rs` |
| Diamonds on timeline clips | create | `timeline_ui.rs` |
| Linear interpolation | exists | also Hold, ease, Bezier already there |

### 8. Shortcuts

| Spec item | Status | Where |
|---|---|---|
| Configurable keymap, disabled in text fields | exists | `settings.rs` (`Action`, `Keymap`) |
| New bindings (Ctrl+B, Q, W, A, B, `[`, `]`, P, N, S, M, J/K/L, ↑/↓, Shift+Z, Ctrl+D, Alt+G…) | modify | today Split = `T`, fast forward = `a`, ripple = `<`, export = `Ctrl+Shift+E`; J/K/L shuttle and ↑/↓ edit jumps to create |

### 9. Export modal

| Spec item | Status | Where |
|---|---|---|
| Modal, cover, name with numeric suffix, `~/Videos/Exports`, Browse | done | `export_modal.rs` (Ctrl+E, blocks the editor) |
| Resolution by short side incl. "match best video" | done | `export_plan.rs` |
| Bitrate modes (match best via ffprobe, recommended, lower, higher, custom CBR/VBR) | done | `VideoSettings::bitrate` in `vv-media/src/encode.rs` |
| Codecs H.264 / HEVC / AV1 | partial | only H.264 is offered (the others are hidden: `CodecChoice::available`); HEVC/AV1 need new encoders in `vv-media` |
| Formats MP4 / MOV | done, untested | MOV chosen from the file extension; AV1 in MOV is already marked invalid |
| Frame rate choice / match best | done | `ExportSettings::fps`, nearest-frame retiming in `vv-session` |
| Audio-only / separate audio MP3/WAV/AAC/FLAC | done | `AudioFileEncoder` |
| Captions SRT/TXT | done | `vv_core::captions::clips_to_srt/txt` |
| Live estimate of duration and size | done | `Estimate` (approximate with VBR) |
| Progress, frame preview, ETA, cancel deletes the partial files, open folder | done | `ExportProgress::preview`, `export_timeline` cleanup |

### 10. Save and empty states

| Spec item | Status | Where |
|---|---|---|
| Autosave 2 s debounce / 30 s max / on close and Home | create | `project_io.rs` (manual Ctrl+S today) |
| Atomic write (temp + rename) | modify | `vv-core/src/persistence.rs` uses `std::fs::write` |
| Crash recovery | create | `project_io.rs` |
| Empty timeline hint | create | `timeline_ui.rs` |

### 11. AI and assets

| Spec item | Status | Where |
|---|---|---|
| Local transcription model | create | none; `plans/TRANSCRIPTION.md` proposes an **external OpenAI-style API**, which contradicts the spec (R6) |
| Auto Cutout, Noise Reduction, Voice Changer, Enhance, Stabilize | placeholder | none present. Local ML that does exist: Silero VAD (`vv-vad`, silence removal) and optical flow (`smooth_cut.rs`) |
| Effects/filters/transitions as folders with JSON manifest + shader | create ⚙ | effects are a closed Rust enum (`FilterKind`, `TransitionKind`) with hand-written WGSL in `vv-render`; no data-driven shader path |
| ~6 transitions, 6 effects, 6 filters | partial ⚙ | 3 transitions (Push, Cross Dissolve, Smooth Cut), 5 filters |

## 2. Risks

**R1 — UI toolkit: movable and detachable blocks.** UI is egui/eframe 0.36
on wgpu. Resizable panels: yes. Drag-to-rearrange: not built in; no docking
crate in the tree. Native detached windows: egui multi-viewport is supported
by eframe on wgpu, and the egui-wgpu renderer is shared across viewports, so
the preview texture (no readback) can be shown in a detached window.
Caveats: deferred viewports need `Send + Sync` state, while the app is
single-owner (`Session` checked out per frame for MCP), so immediate
viewports are the realistic option (each detached window repaints with the
main loop, more CPU when several are open); on **Wayland** windows cannot be
positioned by the app, so "restore on the second monitor" restores the
detachment and size, not the position. Proposal: a small custom tiling
layout (4 known blocks, serialized tree) or `egui_tiles` if a version
compatible with egui 0.36 exists, plus immediate viewports for detached
blocks. To validate with a spike before phase 1.

**R2 — Project schema.** RON with **no format version** by design
(`ARCHITECTURE.md`: old files load through serde defaults). Adding
`format_version` with `#[serde(default)]` keeps every existing file loading
(missing → version 0), then a migration chain on load. Feasible. Two
mismatches with the spec's model: a `Project` holds **N timelines** with
their own fps (compound clips are timelines too), and media are referenced by
absolute path. The spec's "project = one sequence + its own media" maps onto
"one main timeline per project"; the others stay for compounds.

**R3 — Undo/redo.** Command pattern with `History::begin_group` /
`end_group` and `CompositeCommand`; MCP and session jobs go through the same
`History`. Drag gestures and multi-clip property edits are already one step.
Low risk: new operations (Q/W, Linkage, magnet, animations) are compositions
of existing commands; each new gesture must open/close a group.

**R4 — Effects, transitions, keyframes in the engine.** Keyframes are
complete (every parameter, 6 interpolation types). Effects and transitions
are closed enums rendered by dedicated WGSL code paths in
`vv-render/src/compositor.rs`; adding any new visual (chroma key, LUT, HSL,
curves, sharpen, vignette, new transitions, text stroke/curve) or the
manifest+shader asset system means writing compositor code. Things the spec
asks for that can be built **above** the engine: masks of new shapes,
animations, mirror, canvas colour, freeze frame, adjustment-clip effects on
the timeline, transition previews.

**R5 — Encoder.** Export is H.264 + AAC in MP4, quality-based. Bitrate modes,
HEVC/AV1, MOV, export fps, audio-only formats are all in `vv-media/src/encode.rs`
and the export pipeline: engine changes. Without them, the export modal can
only offer H.264/MP4 with quality presets and an approximate size estimate.

**R6 — Local transcription.** No model in the project. The existing plan
picks an external API. For a local, downloaded-on-first-use model the
options are whisper.cpp through `whisper-rs` (C/C++ build dependency, weighs
on the AppImage and on Asahi without GPU), or a pure Rust implementation
(`candle` Whisper), consistent with the choice of `tract` for VAD. Word
timestamps are required for word-by-word templates.

**R7 — AI features.** None of Auto Cutout, Noise Reduction, Voice Changer,
Enhance, Stabilize exists: all placeholders.

**R8 — Spec vs existing behaviour.** Global ripple vs main-track magnet;
linked video+audio clips on separate tracks vs CapCut's audio inside the
video clip; current shortcuts; "no colour correction" in `ARCHITECTURE.md`
although wheels and scopes exist.

## 3. Phase order

Proposed:

1. **Schema + Home + layout**: `format_version` and atomic save first (every
   later phase writes new fields), then home, top bar, blocks, autosave.
   Start with the R1 spike.
2. **Timeline** — unchanged.
3. **Library base** (was 4): the first-media rule and `+`/drag insertion
   feed the timeline; makes phase 2's montage test realistic.
4. **Player + Properties + keyframes** (was 3): mostly reorganising existing
   panels.
5. **Effects/Filters/Transitions + Compound** — scope depends on Q1.
6. **Export** (split from subtitles; scope depends on Q1).
7. **Subtitles** (depends on Q6).
8. **AI placeholders, assets, polish**. Placeholders for unavailable controls
   are added in the phase that builds their UI, not left to the end.

## 4. Questions before phase 1

- **Q1 — Engine boundary.** Many items marked ⚙ (new shaders, codecs, bitrate
  control, containers, audio-only export, reverse, speed curves,
  SVG/animated stickers, text stroke) cannot be done without code in
  `vv-render`/`vv-media`. Is *adding* shaders/codecs to the engine allowed,
  as long as the existing pipeline is not changed? Or do these become
  placeholders?
- **Q2 — Repository language.** `CLAUDE.md` requires English for docs and
  code, so this file is in English; UI strings in Italian go in
  `crates/vv-app/locales/` (rust-i18n, already centralized). OK?
- **Q3 — Audio of video clips.** Keep the current model (video and audio as
  linked clips on separate tracks, shown together) or move to CapCut's
  model (audio inside the video clip until "Extract audio")? The second
  touches the model and the mixer.
- **Q4 — Project on disk.** One folder per project in `~/Videos/Projects`
  (`.vvproj` + copied/linked media), or a single `.vvproj` file referencing
  media where they are? It decides size on disk, duplicate and delete.
- **Q5 — Magnet.** Close gaps only on the Main Track (spec) instead of the
  current global ripple? With Linkage off this desyncs overlays and audio.
- **Q6 — Transcription.** Local `candle` Whisper, `whisper.cpp`, or keep the
  external-API plan?
- **Q7 — Preview quality.** Map Full/½/¼ onto the existing proxies, or
  render the preview at a fraction of the timeline resolution?
- **Q8 — Existing features outside the spec** (mixer, VST3, colour
  wheels/scopes, slip, markers, OTIO, MCP, multiple timelines): keep them
  reachable (menu/secondary panels) or hide them?
- **Q9 — `handoff.md`** is missing: is there one to add?
