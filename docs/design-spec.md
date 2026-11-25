# Design spec – CapCut Desktop style (dark theme)

Visual reference derived from screenshots of CapCut Desktop (Italian UI). All
numeric values (colors, px) are eyeballed estimates: a starting point to refine
in the token file (`crates/vv-app/src/theme.rs`), not exact measurements. This
document covers appearance only; behaviour stays as in the functional spec.

## 1. Principles

- Neutral dark theme in grey levels with a single accent: cyan, for active
  state, selection and primary action.
- High density: small text (11–12 px), compact controls, little decoration. No
  heavy shadows, no gradients except the Home banner.
- Hierarchy by grey level: window (darkest) → panel → raised elements
  (buttons, fields, rows). Thin or no borders: contrast comes from the grey.
- Panels are cards separated by an ~8 px gap, with rounded corners.
- Line icons, thin stroke, monochrome: light grey by default, white on hover,
  cyan when active.
- Text: white primary, mid grey secondary, dark grey disabled.

## 2. Tokens (proposal)

| Token | Value | Use |
|---|---|---|
| bg-window | #101010 | window background, gap between panels |
| bg-panel | #1c1c1c | panels |
| bg-inset | #171717 | timeline track area, player canvas |
| bg-raised | #2a2a2a | secondary buttons, fields, list rows |
| bg-raised-hover | #343434 | hover on raised elements |
| bg-selected | #383838 | active segment, selected entry |
| border-subtle | #2c2c2c | thin separators (1 px) |
| text-primary | #f0f0f0 | main text |
| text-secondary | #9a9a9a | labels, secondary text |
| text-disabled | #5c5c5c | disabled controls |
| accent | #1fc8d6 | cyan: active, selection, primary action |
| clip-video | #0f6e73 | video and image clips (teal) |
| clip-text | #b5503f | text clips (brick red) |
| clip-audio-fx | #2e4f87 | sound effects (blue) |
| clip-audio-music | #123c6e | music (dark blue) |
| clip-selected | #ffffff | 2 px border of the selected clip |
| favorite | #ffc400 | active favorite star |

- Spacing: base 4 px (4, 8, 12, 16, 24). Panel gap 8; panel inner padding 10–12.
- Radii: 2 (tags), 4 (buttons, fields), 6 (panels, menus), 8 (modal, banner).
- Typography: geometric sans (Inter or Roboto). Sizes: 11 (tabs, secondary
  labels), 12 (UI body), 13 (section titles), 14 (modal title). Weights
  400/500, section titles 600.
- Icons: 16 px, 1.5 px stroke, one coherent open-source set (Lucide or Phosphor).

## 3. Window layout

- Top bar ~36 px; below, a row of 3 panels (~50% of the height) and the
  full-width timeline (~43%).
- Top row widths with a project open: Library ≈ 37%, Properties ≈ 30%, Player
  ≈ 32%. Empty project: Library ≈ 40%, Details ≈ 33%, Player ≈ 27%. Resizable.
- 8 px gap between cards, 6 px corners, window background bg-window.

## 4. Top bar

- Background bg-window. Left: Home button and drop-down menus (secondary
  button style, h 24, radius 4). Centre: project name (12 px, white) with the
  save indicator next to it.
- Right: Layout menu, and Export as the primary button (solid cyan, h 24,
  radius 4, upload icon, white text). No "Share" button.

## 5. Library (top-left panel)

- Tab bar (h ≈ 40): 20 px icon above an 11 px label, one per tab. Active tab:
  icon and label cyan; others light grey.
- Left sidebar (≈ 110 px wide, thin separator line): vertical list of buttons
  h 24, bg-raised, radius 4, 12 px text. The selected entry has cyan text.
  Groups have a ⌃/⌄ arrow. The first entry is "Favorites".
- Tool row (h ≈ 32): button with cyan "+" (e.g. "Import"), search field that
  grows, on the right view, sort and filter icons.
- Media grid: thumbnails ≈ 120×80 (radius 3). "Added" tag top-left and
  duration top-right, both on a semi-transparent dark background. File name
  below, 11 px, grey-blue.
- Audio list: rows ≈ 46 px high, bg-raised, radius 6. Left a 36 px square with
  a note icon, then title (12 px white) and below "Author · 00:02" (11 px
  grey). Right: the star (yellow if favorite, outline if not) and a round "+"
  button 22 px #333. On top, pill segments (Music / Sound effects), the active
  one on bg-selected.
- Transitions, Effects, Filters grid: full-width search field on top; cells
  ≈ 88 px square (radius 4), gap ≈ 14 px; label below (11 px, truncated). Star
  and actions appear on hover.
- Media empty state: large rounded bg-raised rectangle with "⊕ Import" (cyan
  circle with +) and below "Drag and drop video, photos and audio files here"
  (11 px grey).

## 6. Properties (top-centre panel)

- Tabs on top: 12 px text, spaced ≈ 24 px, no underline; the active tab is cyan.
- Sub-tabs as a full-width segmented control: bg-raised, radius 4, active
  segment bg-selected.
- Sections: 12 px title (weight 500) with a small arrow; on the right the grey
  icons ↺ reset and ‹ ◇ › (previous keyframe, add, next). 1 px separator
  between sections, vertical padding ≈ 14 px.
- Slider row: label above, below a thin bar (2 px, grey #444) with a round
  ≈ 10 px light-grey knob. On the right a value field (≈ 64 px) with text and
  vertical up/down arrows, then ‹ ◇ ›.
- Numeric fields (X, Y, rotation): bg-raised, radius 4, h 24, small grey X/Y
  letter inside on the left, centred value, arrows on the right. Next to
  rotation a round knob.
- Switch: pill 28×14, cyan if active with a white dot. Checkbox: 14 px, radius
  2, solid cyan with a check if active.
- Alignment row: 6 line icons (left, centre, right, top, middle, bottom) + 2
  disabled.
- CapCut's Pro/Free badges are not reproduced.
- Empty project ("Details" panel): grey 11 px label column (≈ 130 px) and white
  12 px values (Name, Path, Aspect ratio, Resolution, Frame rate...). Bottom
  right an "Edit" button (bg-raised).

## 7. Player (top-right panel)

Panel and header

- Card with header ≈ 40 px. Left the title "Player – <timeline name>" (12 px
  white). Right the menu icon ≡ (three lines, grey) that opens the player menu.

Canvas

- Neutral dark area (bg-inset), no checkerboard. The preview frame is centred
  and scaled to fit the panel, with a ≈ 6–8 px margin. A 9:16 video takes
  almost the full height and leaves dark bands on the sides.
- With a clip selected: 1 px white outline on the clip box and round (≈ 8 px,
  white) handles at the 4 corners. Side and rotation handles are not visible in
  the reference (probably off-frame): check against real behaviour.

Control bar (bottom of the panel, h ≈ 44 px, three zones)

- Left: current timecode in cyan, then a grey " / " and the total duration in
  light grey. Format HH:MM:SS:FF (frames, not hundredths), e.g.
  00:00:00:00 / 00:00:45:27, fixed-width digits, 12 px. Optionally a small grey
  two-column-bars icon (non-essential).
- Centre: Play/Pause as a white triangle only ≈ 14 px, no background circle,
  centred in the panel (becomes pause while playing).
- Right, left to right, spaced ≈ 12 px:
  1. icon in a rounded box with a music note (social safe-zone overlay)
     [placeholder, optional];
  2. "Full": 11 px text button with thin outline; opens the preview quality
     menu (Full, 1/2, 1/4);
  3. fit-to-view icon (magnifier between four corners): resets the canvas to
     the fitted view;
  4. "Ratio": text button with thin outline; opens the menu (Original, 16:9,
     9:16, 1:1, 4:3). The label stays "Ratio", the active value is checked in
     the menu;
  5. fullscreen icon (arrows at corners).
- States: icons text-secondary, white on hover, text-disabled when unusable.
  Empty project: empty canvas, timecode 00:00:00:00 / 00:00:00:00, dimmed controls.
- NOT to reproduce: the floating "EditPilot" pill.

## 8. Timeline

- Toolbar (h ≈ 36), from the left: "+", mouse tool with drop-down arrow,
  undo/redo (grey if unavailable), three cut icons (split, delete left, delete
  right), trash, placeholder. Right: microphone (record voice), then 4 toggles
  with icon (Main Track Magnet, Snapping, Linkage with arrow, Preview Axis with
  arrow): cyan if on, grey if off. Then: fit to window, zoom out, zoom slider
  (thin line, white ≈ 8 px knob), zoom in.
- Ruler (h ≈ 24): ticks and times (00:00, 00:03, ...) in text-secondary, 11 px.
- Playhead: 1 px white line across all tracks with a handle on top (rounded
  pentagon, white outline, dark fill).
- Track header (fixed column ≈ 130 px): grey 12–14 px icons: track type, lock,
  eye (visual tracks only), speaker (tracks with audio), ⋯ (track options). The
  Main Track has on the left a small pencil button ("Edit cover").
- Track order and heights: text on top (≈ 26 px), video overlays above the Main
  Track, Main Track (≈ 52 px), audio below (≈ 52 px each).
- Clips (radius 3):
  - Video/image: teal title strip ≈ 16 px with file name and duration (white,
    11 px), below the thumbnail strip, below that a thin teal waveform.
  - Text: brick red with "A≡" icon and truncated text.
  - Sound effects: blue with title and lighter waveform. Music: dark blue, long,
    with title and waveform.
  - Selected: 2 px white border.
- Empty timeline: strip with dashed border and film icon, text "Drag material
  here and start creating" (12 px grey).

## 9. Context menu

- Width ≈ 300 px, background #262626, radius 6, soft shadow
  (0 8px 24px rgba(0,0,0,.5)).
- Entries ≈ 28 px high, horizontal padding 16, 12 px text. Shortcuts on the
  right as small chips (rounded rectangle with grey border, e.g. "Ctrl" "C").
  Thin separators between groups, disabled entries in text-disabled, › arrow for
  submenus, hover on bg-raised-hover.
- No Pro/Free badges.

## 10. Export modal

- Window ≈ 720×660, bg-panel, radius 8. Title top-left ("Export – <project
  name>", 12 px).
- Two columns. Left: cover preview (≈ 180×320, radius 6) with a dark pill
  "✎ Edit cover" top-left. Right: form with grey 12 px labels (≈ 130 px column)
  and bg-raised controls, h 28, radius 4; menus have the arrow on the right.
- Rows: Name (text field), Export to (path + folder button), separator, Video
  section (cyan checkbox + arrow), Resolution, Bit rate, Codec, Format, Frame
  rate, Colour space (disabled row, e.g. "Rec. 709 SDR"). The right column
  scrolls with a thin scrollbar.
- Footer (h ≈ 56, line above): left film icon + "Duration: 46s | Size: about
  34 MB" + small info circle; right "Export" (solid cyan, h 28, radius 4) and
  "Cancel" (bg-raised).
- The extra options of our spec (Match the best video, CBR/VBR, audio,
  subtitles) use the same components: same label + control row.

## 11. Home

- Window background with a slight gradient toward a very dark teal at the top
  (#0b1618 → bg-window). Left sidebar ≈ 175 px: entries "Home" (selected:
  #1f1f1f, radius 4) and "Library". No account, Pro, AI entries or invites.
- Banner "＋ Create a project": full width, h ≈ 145, radius 8, horizontal cyan
  gradient (#26c6da → #8fe3f0), dark 20 px bold text with the "+" in a small
  square. Replaces the plain "New project" button: same behaviour (creates and
  opens at once, asks nothing).
- "Projects" section: 13 px bold title; on the right search, grid/list view
  (menu) and trash icons.
- Grid: square thumbnails ≈ 124 px (radius 3), horizontal gap ≈ 22 px, row
  ≈ 178 px. Below the thumbnail: name (12 px, bold, white) and "size | duration"
  (11 px grey, e.g. 180.4M | 00:29). A project without media has a black
  thumbnail.
- AI cards (AI Video, EditPilot, etc.) and the "Creative suite" section are not needed.

## 12. Not to copy

CapCut logo and name, Pro/Free badges and purple diamonds, the EditPilot pill,
AI cards and functions, sample thumbnails and templates (e.g. "PAUSE",
"ONFIRE"), music and transition libraries, proprietary icons. Copy the style,
not the assets.

## 13. Differences to align with the functional spec

1. Player: text buttons are "Full" (preview quality) and "Ratio"; timecode is
   HH:MM:SS:FF; menu icon ≡ in the header; Play centred without circle.
2. Home: project creation is a big banner on top; cards show size and duration.
3. Empty project: instead of Properties, a "Details" panel with Aspect ratio,
   Resolution, Frame rate and "Edit".
4. Export: thumbnail with "Edit cover" and a "Colour space" row.
5. Timeline: header also has the "⋯" track-options menu (content TBD); text
   track on top.

## Rule for new UI

Every new piece of UI uses only the tokens from `theme.rs` and follows this
document. No colour, radius, spacing or font size is hardcoded elsewhere.
Illustrations (thumbnail art, EQ band colours) are the only exception.

Shared pieces live in `crates/vv-app/src/ui_kit.rs`; use them instead of
rebuilding the look by hand:

- `heading` for dialog and page titles (14 px, primary text);
- `section_title` for groups of controls (13 px, primary text);
- `hint` for secondary lines (11 px, secondary text);
- `primary_button` for the one solid-cyan action of a view (Export, Apply…);
- `status` with `Tone::{Error, Warning, Success}` for messages.

Spacing uses the `SPACE_*` scale only (4, 8, 12, 16, 24); text sizes use
`FONT_TINY` (graph labels), `FONT_SMALL`, `FONT_BODY`, `FONT_SECTION`,
`FONT_MODAL_TITLE`, `FONT_LARGE` and `FONT_DISPLAY` (banners, full-panel
notices).
