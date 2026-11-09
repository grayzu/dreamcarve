# Phase 4: Experience and visual design language

Status: draft, written 2026-11-02 for the phase 4 design spec; the design
spec's "Visual design" section points here. It designs how the app in
`functional.md` looks and behaves, inside that spec's capabilities (50–80);
it adds no feature and renames none. The wireframes in `mockups/` fix layout,
labels and field names; this document fixes everything they leave grey.
Vocabulary is `CONTEXT.md`'s. Realised in egui + wgpu; the realisation notes
(section 12) say how, and what egui cannot do.

## 1. Principles

1. **A current desktop tool, not a dialog-box program.** Every panel edits
   the project in place, applies as it is made, and is undone from Edit.
   *Rules out:* bevelled or gradient buttons, nested modals, tabbed property
   sheets, OK/Cancel/Apply rows, 16-px icons on a grey toolbar strip,
   inconsistent spacing between panels, a "Properties" window that floats
   apart from the thing it edits.
2. **Numbers are the interface.** Every length and feed is a value, a unit
   and a validation state, aligned so columns of them read as a table.
   *Rules out:* proportional figures in value fields, units in the label
   instead of after the value, bare numbers whose unit the user must infer,
   red text as the only sign that a field blocks export.
3. **One severity ladder, everywhere.** Error (export refuses) is red with a
   filled glyph; warning is amber with an outlined glyph; hint is blue-grey
   with a dot. The ladder reads the same in a field, a row, the status bar
   and the Warnings panel. *Rules out:* a second red (operation colour 2 is
   red and must never be mistaken for an error), coloured text without a
   glyph, warnings that look like hints.
4. **The Viewport is the picture of what the machine will do.** It draws
   what `--preview` draws, at screen-constant line weights, on a calm
   background that lets eight operation colours stay distinct. *Rules out:*
   decorated grids, drop shadows on geometry, 3D-looking stock, chart-like
   gradients, anything the CLI's SVG would not show.
5. **Density of a working tool.** 13-px text, 26-px controls, 28-px rows,
   8-px gutters: a Figma or Fusion density, not a phone app and not a
   spreadsheet. *Rules out:* 36-px touch buttons on the desktop, 11-px text
   in a status bar, padding that varies by panel.
6. **Light and dark are one design.** Every token has two values chosen
   together; the operation palette is checked on both backgrounds.
   *Rules out:* a dark mode made by inverting, a white Viewport in dark mode,
   colours that pass in one mode only.

## 2. The workspace

The window is three regions and a status bar, read as one surface with the
Viewport sunk into it. Chrome is quiet so the stock and its toolpaths are
the brightest thing on screen.

- **Surfaces.** The window background (`surface.window`) shows only as the
  4-px gaps between regions and as the dividers. Side panels sit on
  `surface.panel`. The Viewport is `viewport.bg`, which in both modes is
  slightly *lighter* than the panels in light mode and slightly *darker*
  than them in dark mode, so it reads as the stage. Popovers, menus, the
  visibility card and sheets are `surface.raised` with a 1-px
  `border.default` and a soft shadow. Fields and lists are
  `surface.sunken`.
- **Hierarchy.** Three levels only: panel title (Design Tree, Properties,
  Operations), section header (Shapes, Depth, Strategy), row. No fourth.
  Titles are 13-px SemiBold; section headers are 13-px SemiBold on a
  `surface.panel` band with a chevron; rows are 13-px Regular.
- **Panel chrome.** No panel has a border on its own. A panel is its header
  band (32 px) plus content; the 1-px `border.default` line under the
  header is the only rule. Left column: Design Tree over Properties,
  split by a draggable horizontal divider. Right column: the CAM panel
  with its two tabs (Operations, Tools) in a 32-px tab strip, each tab a
  list over an editor, split by a draggable divider.
- **Dividers.** 4-px wide, `surface.window`; on hover the central 2 px turn
  `accent` after 150 ms; cursor is the platform's resize cursor. Panels
  collapse to a 32-px strip (icon + vertical title) when dragged below
  their minimum or from the panel menu, and reopen from the strip.
- **Menu bar and toolbar.** A 28-px menu bar (egui's own; on macOS the
  design spec may move it to the native bar later: nothing here depends on
  it). Below it a 40-px toolbar on `surface.panel`: quiet buttons with a
  20-px icon and a 13-px label (Open, Save, Undo, Redo, Import, Picture,
  Fit, Tab tool), a 16-px gap between the groups, and Export as the one
  primary button at the right end. The file name and the unsaved dot sit
  right of centre in `text.secondary`.
- **Status bar.** 24 px, `surface.panel`, top rule `border.default`. Cells
  left to right: units, stock size, cursor X Y (tabular), generation state,
  warnings and errors (each a glyph + count in its status colour, a button
  that opens the Warnings drawer), estimated time, Z range when extents
  are on. Cells are separated by 1-px `border.default` rules with 12-px
  padding; cell text is 12 px.
- **Warnings drawer.** Slides up from the status bar over the Viewport's
  bottom 240 px (resizable, min 120), `surface.panel`, top rule; never a
  window.
- **Persistent vs transient.** Persistent: the four regions, the status
  bar, the visibility card (a floating card in the Viewport's top-right,
  collapsible to its title). Transient: menus, the Add-operation menu,
  the tool picker popover, tooltips, the help line, the marquee,
  insertion indicators, toasts for Undo. Sheets (import, export, Tool
  Library, unsaved-changes) are transient and modal; everything else is
  not.
- **Three side panels.** Open question 3 asks that a third side panel fit
  without redesign: the right column is a tab strip over panels, so a
  third tab is the seam; nothing in this document assumes two.

## 3. Layout grid and spacing

A 4-pt scale. Everything snaps to it; the only 2-px uses are hairline gaps.

| Step | px | Used for |
| --- | --- | --- |
| 0.5 | 2 | Gap between a glyph and its count; focus-ring offset |
| 1 | 4 | Region gutters, divider width, gap between icon and label inside a row, vertical gap between stacked controls |
| 2 | 8 | Control inner padding (horizontal), gap between a label and its field, list row horizontal padding, gap between adjacent buttons |
| 3 | 12 | Panel content margin, status-bar cell padding, gap between a field and its validation line's bottom |
| 4 | 16 | Gap between toolbar groups, gap between sections inside a sheet, card padding |
| 6 | 24 | Sheet margin, empty-state card padding (inner) |
| 8 | 32 | Header heights, empty-state card gap between blocks |

Heights (all at 1× scale; egui multiplies by `pixels_per_point`):

| Element | Height | Notes |
| --- | --- | --- |
| Menu bar | 28 | |
| Toolbar | 40 | Buttons 28 tall inside |
| Panel header / tab strip | 32 | |
| Section header | 28 | Full-width hit area |
| Control (field, dropdown, button, segmented) | 26 | Text baseline centred |
| Primary button in a sheet footer | 28 | The one taller control |
| Checkbox / radio glyph | 16 | Inside a 26-px row |
| Design Tree row | 26 | One line |
| Operations and Tools list row | 40 | Two lines: name, then tool or size |
| Table row (Tool Library, export programs, Warnings) | 28 | One line |
| Status bar | 24 | |
| Visibility card row | 24 | |
| Collapsed panel strip | 32 wide | |

Widths: left column default 280, min 240; right column default 340, min
300; Viewport min 560 (so 1280 × 768 holds both panels at minimum, spec
50). Labels in editors take a 112-px column; the field fills the rest;
the unit occupies the last 32 px inside the field. Tool-Library sheet
880 × 600 default, min 720 × 480; Import and Export sheets 560 wide;
unsaved-changes sheet 420 wide.

## 4. Typography

**UI face: Inter**, SIL Open Font License 1.1, copyright 2016 The Inter
Project Authors, no Reserved Font Name
(https://github.com/rsms/inter; verified against `LICENSE.txt` on
2026-11-02). Why Inter: a tall x-height and open apertures at 12–13 px,
figures designed for tables, and a wide Latin set for phase 8's
translations; it is the face Figma and Linear ship with, so it reads as a
current tool. Bundle the **static** instances `Inter-Regular.ttf`,
`Inter-Medium.ttf`, `Inter-SemiBold.ttf` (not the variable font: egui's
rasteriser renders only a variable font's default instance). Inter's
default figures are proportional and egui cannot select OpenType features,
so the bundled files are feature-frozen with `tnum` baked in (OpenType
Feature Freezer, MIT); the OFL permits this and, with no Reserved Font
Name, the family name stays. `assets/fonts/inter/README.md` records the
freeze command and version. Add Inter to `NOTICE`.

**Monospace: JetBrains Mono**, SIL Open Font License 1.1, copyright 2020
The JetBrains Mono Project Authors, no Reserved Font Name
(https://github.com/JetBrains/JetBrainsMono; verified against `OFL.txt`
on 2026-11-02). Bundle `JetBrainsMono-Regular.ttf`. Distinct `0/O`,
`1/l/I`, and a width that keeps warning codes and G-code aligned. Add to
`NOTICE`.

Type scale (px at 1×; line height 1.4 unless noted):

| Role | Face | Size | Weight | Colour | egui `TextStyle` |
| --- | --- | --- | --- | --- | --- |
| Empty-state heading, sheet title | Inter | 18 | SemiBold | text.primary | `Name("Display")` |
| Heading inside a sheet | Inter | 15 | SemiBold | text.primary | `Heading` |
| Panel title, tab label (active) | Inter | 13 | SemiBold | text.primary | `Name("Title")` |
| Section header | Inter | 13 | SemiBold | text.primary | `Name("Title")` |
| Row name, body, button label | Inter | 13 | Regular | text.primary | `Body`, `Button` |
| Emphasised row name (selected row) | Inter | 13 | Medium | text.primary | `Name("Medium")` |
| Field label | Inter | 13 | Regular | text.secondary | `Body` |
| Field value | Inter (tnum) | 13 | Regular | text.primary | `Body` |
| Unit suffix | Inter | 12 | Regular | text.secondary | `Small` |
| Placeholder (`tool's: 3 mm`) | Inter | 13 | Regular, italic by colour only | text.disabled | `Body` |
| Row second line (tool, size), help line, validation line | Inter | 12 | Regular | text.secondary / status | `Small` |
| Status bar | Inter (tnum) | 12 | Regular | text.secondary | `Small` |
| Warning code, file name, G-code, `.dcv` field name in a tooltip | JetBrains Mono | 12 | Regular | text.primary | `Monospace` |
| Cursor coordinates, depth-legend labels, Viewport labels | Inter (tnum) | 12 | Regular | text.secondary | `Small` |

Rules: no uppercase tracking, no italic (Inter Italic is not bundled;
placeholders differ by colour), no text under 12 px. Numbers in every list
column and field use tabular figures by construction (the frozen font).
Negative Z is written with a true minus (`−12.2 mm`, U+2212), and
thousands have no separator. Units follow the value after a thin space in
the same field: `6 mm`, `0.25 in`, `1200 mm/min`, `18000 rpm`, `45°`,
`40 %`.

## 5. Colour

All values sRGB hex. Contrast ratios are WCAG 2 against the surface named
and were computed, not estimated. "Visuals" names the egui field the token
sets (section 12).

### Surfaces, borders, text, accent

| Token | Light | Dark | Visuals field |
| --- | --- | --- | --- |
| surface.window | `#ECEEF1` | `#15181D` | `window_fill` of the root, `Frame::none().fill` of the central area |
| surface.panel | `#F7F8FA` | `#1E2229` | `panel_fill` |
| surface.raised | `#FFFFFF` | `#262B33` | `window_fill` (popups, sheets), `widgets.open.bg_fill` |
| surface.sunken | `#FFFFFF` | `#121519` | `extreme_bg_color` (text edits, lists) |
| surface.stripe | `#F2F3F6` | `#22262E` | `faint_bg_color` |
| surface.hover | `#1C212814` (8 % of text.primary) | `#E8EBF014` | `widgets.hovered.weak_bg_fill` |
| surface.active | `#1C212820` | `#E8EBF020` | `widgets.active.weak_bg_fill` |
| border.default | `#D7DBE2` | `#333A44` | `widgets.noninteractive.bg_stroke`, `window_stroke` |
| border.strong | `#B9C0CA` | `#48515D` | `widgets.inactive.bg_stroke` (field outline) |
| text.primary | `#1C2128` (15.2:1 on panel) | `#E8EBF0` (13.4:1) | `widgets.*.fg_stroke.color`, `override_text_color` unset |
| text.secondary | `#5B6472` (5.6:1) | `#A3ABB7` (6.9:1) | `widgets.noninteractive.fg_stroke` for labels (set per widget) |
| text.disabled | `#9AA3AF` | `#6B7380` | `widgets.noninteractive` when `!enabled` |
| text.on-accent | `#FFFFFF` | `#FFFFFF` | set per widget |
| accent | `#2563D0` (5.2:1 on panel; 5.6:1 with white text) | `#6AA3F2` (6.2:1) | `hyperlink_color`, `selection.stroke` |
| accent.fill | `#2563D0` | `#2F6FD6` (4.8:1 with white text) | primary button fill |
| accent.soft | `#2563D01F` | `#6AA3F229` | `selection.bg_fill` (selected rows, text selection) |
| focus.ring | `#2563D0` | `#6AA3F2` | custom (12) |

Rationale for the blue accent: the operation palette's first colour is also
blue (`#1F77B4`), but the accent appears only on chrome (rows, rings,
handles), never as a toolpath, and the Viewport's selection colour is the
accent at 2 px while toolpath 1 draws at 1.5 px in a different, darker
blue. The two never meet on the same element: a selected shape is the
*shape* turning accent, not a toolpath.

### Status

Each status colour has a text value (meets 4.5:1 on panel in its mode), a
stroke value for glyphs and field outlines (≥3:1), and a tint for row and
field backgrounds. Never used without a glyph or a word.

| Token | Light text / stroke / tint | Dark text / stroke / tint | Glyph (Lucide) | Visuals |
| --- | --- | --- | --- | --- |
| status.error | `#C62E27` / `#C62E27` / `#C62E2714` | `#F2726A` / `#F2726A` / `#F2726A1F` | `circle-x` filled | `error_fg_color` |
| status.warning | `#A3580A` / `#D98A1E` / `#D98A1E1A` | `#F0B347` / `#F0B347` / `#F0B3471F` | `triangle-alert` outline | `warn_fg_color` |
| status.info (hint) | `#2A6CC4` / `#2A6CC4` / `#2A6CC414` | `#78B0F5` / `#78B0F5` / `#78B0F51F` | `info` outline, or a 6-px dot in a count | custom |
| status.success | `#1F7F43` / `#1F7F43` / none | `#5FC67F` / `#5FC67F` / none | `circle-check` | custom |

The light warning *text* is darker than its stroke so the message under a
field passes 4.5:1 while the outline and triangle keep the amber the eye
expects (`#D98A1E` is 2.6:1 on panel, so it is never used for text).

### Viewport

| Token | Light | Dark | Contrast on viewport.bg |
| --- | --- | --- | --- |
| viewport.bg | `#FAFAF8` | `#1B1F26` | — |
| viewport.grid.minor | `#ECEEF1` | `#232830` | 1.1:1 (deliberately faint) |
| viewport.grid.major | `#D9DCE2` | `#2C323B` | 1.3:1 |
| viewport.axis (origin crosshair, X/Y axis lines through it) | `#6B7380` | `#9AA3AF` | 4.6 / 6.5 |
| viewport.stock (outline, dashed) | `#6B7380` | `#9AA3AF` | 4.6 / 6.5 |
| viewport.stock.fill | `#FFFFFF` | `#1F242C` | the stock is a shade off the background so its extent reads without the dashes |
| viewport.male-board (outline, origin mark, mirrored shapes) | `#8A929E` | `#7B8491` | 3.0 / 4.4 |
| viewport.shape (closed, solid) | `#23272E` | `#E6E9EE` | 14.3 / 13.6 |
| viewport.shape.open (dashed, same colour) | as shape | as shape | |
| viewport.shape.hover | `#5B8DE8` | `#8FBAF7` | 3.1 / 8.3 |
| viewport.shape.selected | `#2563D0` | `#6AA3F2` | 5.3 / 6.4 |
| viewport.shape.locked | `#8A929E` + padlock glyph | `#7B8491` + padlock | 3.0 / 4.4 |
| viewport.shape.hidden | not drawn | not drawn | |
| viewport.picture | the bitmap at 100 %, 60 % when any shape is selected | same | |
| viewport.rapid (thin dashed) | `#8A929E` | `#6E7786` | 3.0 / 3.7 |
| viewport.plunge (mark at a descent) | `#C62E27` | `#F2726A` | 5.3 / 5.8 |
| viewport.tab (heavy mark) | `#1C2128` | `#F5F7FA` | 15.5 / 15.4; a hollow (uncrossed) tab is the same stroke at 1.5 px with no fill |
| viewport.start (circle + tick) | the operation's colour | same | |
| viewport.drill (bit-sized circle) | the operation's colour | same | |
| viewport.direction-arrow | the operation's colour | same | |
| viewport.extents.centre (solid) | `#5B6472` | `#A3ABB7` | 5.7 / 7.1 |
| viewport.extents.edge (dashed, outside) | `#8A929E` | `#7B8491` | 3.0 / 4.4 |
| viewport.park (crossed circle) | `#5B6472` | `#A3ABB7` | |
| viewport.marquee | accent at 1 px, accent.soft fill | same | |
| viewport.handle (fill / stroke) | `#FFFFFF` / `#2563D0` | `#1B1F26` / `#6AA3F2` | |
| viewport.label.bg (cursor readouts, extents caption) | `#FFFFFFE0` | `#262B33E6` | |

### Operation colours

The CLI's `PALETTE` (`crates/dreamcarve/src/preview.rs`) is Matplotlib's
tab10, chosen on white. Three of its eight (orange, cyan, pink) sit at
2.2–2.6:1 on the light Viewport, under the 3:1 floor for a 1.5-px line,
and brown drops to 2.8:1 on the dark one. **Proposal:** keep the eight
hues and their order (so the quick-start pictures and the CLI's SVG still
match the screen by eye), tune four light values up to ≥3.3:1, and give
dark mode its own set. The light set is the one the CLI should adopt too
if the maintainer accepts the change (one constant table in one crate;
acceptance test 79 compares the SVG dump to `--preview`, so the dump must
use whichever table the CLI uses). If the maintainer keeps the CLI's
values, the light Viewport draws the CLI's hexes unchanged and only the
dark set is new; nothing else in this document moves.

| # | Name | CLI (light today) | Light (proposed) | on `#FAFAF8` | Dark | on `#1B1F26` |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Blue | `#1F77B4` | `#1F77B4` | 4.6 | `#5BA6EA` | 6.4 |
| 2 | Red | `#D62728` | `#D62728` | 4.8 | `#F0605A` | 5.1 |
| 3 | Green | `#2CA02C` | `#2A8F2A` | 4.0 | `#5FC95F` | 7.9 |
| 4 | Purple | `#9467BD` | `#8E5FB8` | 4.5 | `#B48FE0` | 6.3 |
| 5 | Orange | `#FF7F0E` | `#D96A00` | 3.3 | `#FFA046` | 8.2 |
| 6 | Cyan | `#17BECF` | `#0E98A8` | 3.3 | `#4FD4E4` | 9.3 |
| 7 | Brown | `#8C564B` | `#8C564B` | 5.7 | `#C8907F` | 6.1 |
| 8 | Pink | `#E377C2` | `#C94FA8` | 3.9 | `#F08FD2` | 7.5 |

Operation 2 is red in both modes because the CLI's is, and the Warnings
ladder depends on red meaning *error*. The rule that keeps them apart:
status red appears only on chrome with a glyph; toolpath red appears only
as a line on the Viewport, never as text, never with a glyph. A V-Carve's
clearing-pass floor rings draw in the operation's colour at 55 % alpha
(spec 70: "lighter than the V-bit's lines").

### Depth ramp ("By depth")

Five stops interpolated in linear sRGB, pale at the stock top to deep at
the deepest cut. On dark the "dark" end is a saturated blue rather than a
black one, so the deepest cut still stands off the background; the hue
family is the same, so the legend reads the same in both modes.

| Stop | Light | Dark |
| --- | --- | --- |
| 0 (stock top) | `#AFC2EE` (1.7:1; pale by intent, the ramp's top is rarely cut) | `#E4ECFF` (14:1) |
| 0.25 | `#7C98E0` | `#BCCBF8` |
| 0.5 | `#4A68C9` (3.7:1) | `#93A8F2` (6.3:1) |
| 0.75 | `#2A44A6` | `#6E86EE` |
| 1 (deepest) | `#1A2E8C` (11:1) | `#4A66E8` (3.4:1) |

The legend bar (section 9) is the same ramp drawn vertically, 12 px wide.

## 6. Iconography

**Set: Lucide**, ISC licence, copyright 2026 Lucide Icons and
Contributors, with the Feather-derived icons under MIT (Cole Bemis)
(https://github.com/lucide-icons/lucide; verified against `LICENSE` on
2026-11-02). Design rules: 24 × 24 grid, 2-px centred stroke, round caps
and joins, 2-px corner radius, 1-px inner padding. Bundle the icon font
`lucide.ttf` and its codepoint map from the `lucide-static` package, loaded
as the egui font family `icons`; icons draw at **16 px** in rows, fields,
section headers and the status bar, and **20 px** in the toolbar and sheet
headers. Tint is the text colour of the context (text.primary, secondary,
or a status colour); never a colour of their own. Add Lucide to `NOTICE`.

Operation-kind glyphs are not in Lucide; they are DreamCarve's own, drawn
on the same grid and rules, shipped as painter code (section 12) so they
can take the operation's colour. Each is readable at 16 px:

| Kind | Glyph |
| --- | --- |
| Profile | A rounded square outline with a second, offset outline on its outside right and bottom (the cut beside the line) |
| Pocket | A rounded square outline filled with three horizontal raster lines |
| Drill | A circle with a centred dot |
| Bore | A circle with a smaller concentric circle and a short spiral tick inside |
| Engrave | A single open wavy line, no closed shape |
| V-Carve | A V (two strokes meeting at the bottom) above a short baseline |
| Inlay | Two mirrored V shapes nested, the lower one inverted (the plug in its pocket) |
| Photo V-Carve | A rounded rectangle with four short diagonal hatch strokes |

The same glyph marks the operation row, the Add menu, the editor's title
and the status-bar "Generating" cell.

Tree and row glyphs (Lucide names): visible `eye`, hidden `eye-off`;
locked `lock`, unlocked `lock-open` (shown only on hover or when locked);
linked copy `link-2`; open path a 12-px text badge `open` on
`surface.stripe`, not an icon; drag grip `grip-vertical`; section chevron
`chevron-right` rotating to `chevron-down`; remove target `x`; reset to
tool's `rotate-ccw`; help `circle-help` (section 7 says when it shows);
picture row `image`; tab row `bookmark`; tool types end mill `cylinder`,
ball nose `circle-dot`, V-bit `triangle` (point down), engraver
`pencil-line`, drill `move-down`.

Status glyphs: error `circle-x` **filled** (the only filled status glyph,
so it is also the heaviest), warning `triangle-alert`, hint `info`,
success `circle-check`, generating a 14-px arc spinner (section 10),
stale a 35 % alpha wash with no glyph, unused-tool hint `info` in the
row's status cell.

Toolbar: Open `folder-open`, Save `save`, Undo `undo-2`, Redo `redo-2`,
Import `file-input`, Picture `image-plus`, Fit `maximize`, Tab tool
`bookmark-plus`, Export `download`. Visibility card: Show all `eye`,
Hide all `eye-off`. Viewport: 1:1 `scan`, zoom to selection `focus`.

## 7. Controls

Shared state language (applies to every control below):

| State | Look |
| --- | --- |
| default | `surface.sunken` fill, 1-px `border.strong` outline, 4-px radius, text.primary |
| hover | outline → text.secondary; buttons: `surface.hover` wash |
| focus | 2-px `focus.ring` outline outside the control, 2 px offset, no change to the inner outline; fields also show the caret |
| active (pressed / open) | `surface.active` wash; dropdown chevron rotates |
| disabled | fill unchanged, outline → `border.default`, text → text.disabled, no hover; a tooltip gives the reason when the spec names one (tool-change Z on a post without `G53`) |
| error | outline → status.error stroke; a 12-px message under the field with `circle-x`; the unit suffix stays grey |
| warning | outline → status.warning stroke; message with `triangle-alert` |
| dimmed-but-editable | the operation with no targets (spec 59) and a disabled operation's row: text.secondary everywhere, controls fully live; never the disabled look |

Controls:

- **Text field with trailing unit.** 26 px tall, 8-px padding, value left
  in tabular figures, the unit right-aligned in text.secondary inside the
  outline. Typing `0.5 in` in a mm project converts on leave and the field
  shows `12.7 mm`; the unit never changes with the input. Validation on
  leave (Tab, Enter, click elsewhere); Escape reverts. One undo step per
  leave.
- **Tool-derived placeholder.** When the file holds `null`, the field is
  empty and shows `tool's: 3 mm` in text.disabled; the unit suffix stays.
  A `rotate-ccw` quiet icon button (16 px) appears inside the field's
  right end only when a value is set, and clears back to `null` (one undo
  step: *Reset step-down of "Panel pocket"*). Tooltip: *Use the tool's
  value*.
- **Numeric entry.** Same field; accepts the file's unit strings (`1/4 in`,
  `1 1/4 in`, `6.35`, `6,35` is rejected with *Use a dot*), degrees and
  percentages with their suffix. Arrow up/down step by 1 (Shift 10, Alt
  0.1) in display units and commit on leave.
- **Dropdown.** 26 px, chevron `chevron-down` right, opens a `surface.raised`
  list with 26-px rows, the current row ticked with `check`; keyboard:
  Space opens, type to jump, Enter commits. The tool dropdown shows the
  tool-type glyph, name, size and `(T2)`; its last row is *Add from
  library…*.
- **Checkbox.** 16-px box, 3-px radius, `border.strong`; checked fills
  `accent` with a white `check`; label 8 px right; the whole row is the
  hit target. No tri-state in this phase.
- **Segmented control.** For 2–4 mutually exclusive options whose labels
  fit in 56 px each: Direction (Climb / Conventional), Z zero (Top /
  Bottom), colour mode (By operation / By depth), Strategy (Offset /
  Raster), Wall pass (Last / First / None), Mirror (two icon segments),
  Machining order. 26 px, segments share one `border.strong` outline; the
  selected segment is `surface.raised` with text.primary Medium and a 1-px
  `border.strong` around it; the rest text.secondary. Longer sets (Side:
  five values; Entry; Start point; Corner relief) use a dropdown. Radio
  buttons do not appear in the app.
- **Toggle.** Only for on/off *features* with sub-fields (Lead, Last pass,
  Cut-out, arc-fit tolerance "off"): a 28 × 16 switch, `border.strong`
  track, `accent` when on; the sub-fields indent 16 px below it and dim
  when off. Elsewhere a checkbox.
- **Disclosure section header.** 28 px, full width, `surface.panel`,
  13-px SemiBold, `chevron-right` 16 px left; open state remembered per
  group. A located warning or error inside a *closed* section shows its
  glyph + count at the header's right end so nothing hides.
- **List row (operations, tools).** 40 px: `grip-vertical` in
  text.disabled (text.secondary on row hover), enable checkbox, kind glyph
  in the operation's colour (a 10-px colour swatch is drawn in the glyph's
  stroke), name 13 px, second line 12 px text.secondary (`Pocket · 6.35 mm
  end mill`), then `eye`, then a 24-px status cell. Hover: `surface.hover`.
  Selected: `accent.soft` fill and a 2-px `accent` bar on the left edge.
  Disabled operation: name in text.secondary, strike nothing.
- **Status cell.** One of: nothing; spinner; `triangle-alert` + count in
  status.warning; `circle-x` filled in status.error; `info` in status.info
  for an unused tool or the tool's red dot (spec 63: a 8-px filled
  status.error dot when a feed is missing). Hover lists the messages.
- **Design Tree row.** 26 px: `eye`, `lock` (hover-revealed unless locked),
  name, then badges (`open`, `link-2`), then the target count right-aligned
  in text.secondary (hover lists the operations). Section headers (Shapes,
  Tabs, Pictures) carry their own `eye` and `lock` acting on all rows.
- **Buttons.** Primary: `accent.fill`, white text, 26 px (28 in sheet
  footers), used once per surface (Export, Add to project, Import in a
  sheet). Secondary: `surface.raised`, `border.strong`, text.primary
  (Duplicate, Place tabs, Add from library…). Quiet: no outline, text or
  icon only, `surface.hover` on hover (toolbar, row actions, Show all /
  Hide all). Destructive: secondary styling with status.error text and
  outline on hover only (Delete, Remove); never red at rest. Icon-only
  buttons are 26 × 26 with a tooltip after 500 ms.
- **Inline validation message.** 12 px, under the field, 4 px gap, glyph
  then text, in the status's text colour; the file's message verbatim
  (`inlay_male_board_unset`'s message, not its code). The code appears in
  the Warnings panel, not here. One message per field; errors win over
  warnings.
- **Help affordance.** The wireframes' "?" circle is replaced: hovering a
  field's *label* for 500 ms shows the chapter's one-sentence definition as
  a tooltip, with the `.dcv` field name in JetBrains Mono on a second line;
  the label's underline (1 px dotted, text.disabled) appears on hover to
  say it has help. `circle-help` is kept only in sheet headers as a link to
  the chapter.
- **Tabs (CAM panel).** 32-px strip, two equal segments, label 13 px;
  active: text.primary SemiBold and a 2-px `accent` underline; inactive:
  text.secondary; a count badge (`Operations · 5`) in text.secondary.
- **Search field.** A text field with a leading `search` glyph and a
  trailing `x` clear when non-empty; filters as typed, 150 ms debounce.
- **Table rows (Tool Library, export programs, Warnings).** 28 px, striped
  with `surface.stripe`, header row 28 px text.secondary 12 px with
  `border.default` under it, columns resizable, numbers right-aligned
  tabular, the tick column 32 px. "Already in project" rows show a `check`
  in text.secondary and the row in text.secondary.

## 8. Interaction patterns

- **Selection.** One selection, shared by the Design Tree, the Viewport
  and the Properties panel. Click selects; Shift-click adds (tree: range);
  Ctrl/⌘-click toggles; a marquee on empty stock selects what it touches;
  Escape clears. The tree scrolls to show the first selected row when the
  Viewport selects. An operation row's selection is a second, independent
  selection (what the editor shows); clicking a toolpath sets it.
- **Hover.** Shapes under the cursor turn `shape.hover` at 1.25 px and the
  status bar shows the name; rows take `surface.hover`; hovering an
  operation's Targets group turns its targets `shape.hover` on the
  Viewport; hovering a tree row's count highlights that operation's
  toolpath by dimming the others to 35 %.
- **Drag to move.** Press and move 4 px on a selected shape starts the
  drag; the shape moves live; the status bar shows ΔX ΔY; release commits
  one undo step. Locked: the cursor is `not-allowed`, nothing moves, the
  status bar says *"panel" is locked*.
- **Drag to reorder (operations, clearing passes).** The grip is the handle;
  the row lifts (shadow, 96 % alpha) and follows the cursor; a 2-px
  `accent` insertion line with a 6-px dot at its left end shows the drop
  slot between rows; release commits *Reorder operations*. Auto-scroll at
  the list's edges.
- **Transform handles.** A selected shape (or the selection's bounding box)
  draws a 1-px `accent` box, four corner handles (8-px squares,
  `handle.fill` / `handle.stroke`), and a rotation handle (8-px circle on
  a 20-px stem above the top-centre). Corner drag scales about the centre
  keeping proportions (Alt frees them); rotation drag shows the angle in a
  label by the cursor, snapping to 15° with Shift. Locked: handles drawn
  in `shape.locked`, no cursor change.
- **Tab dragging.** A placed tab is a 10-px-wide mark across the toolpath;
  hover widens it 2 px and shows a `grab` cursor; drag slides it along
  the outline (projected to the nearest outline point, never off it); the
  Properties panel shows its width, height, shape. The Tab tool arms the
  cursor with a `+`; clicking an outline adds a tab; Escape disarms.
- **Inline rename.** Double-click a tree or operation name: the text turns
  into a field in place with the name selected; Enter commits, Escape
  reverts, blur commits. One undo step.
- **Undo feedback.** Edit shows *Undo Set depth of "Outside cut"*. On
  Undo/Redo a 2-second toast slides in at the Viewport's bottom-centre
  (`surface.raised`, 13 px): *Undone: Set depth of "Outside cut"* with the
  affected operation's row flashing `accent.soft` once (300 ms). The
  Viewport and the editor update in the same frame.
- **Stale and regenerating.** The moment a change settles (250 ms after the
  last edit), the affected rows show the spinner and their old toolpaths
  drop to 35 % alpha; when a result lands, the new toolpath replaces it
  at full alpha with no fade. Nothing is hidden while waiting.
- **Progress.** The status bar's generation cell reads *Generating 2 of 5…*
  with the spinner; a *Stop* quiet button appears beside it while any
  operation is pending; *Regenerate all* is in View. Import and export run
  inside their sheet with a 4-px progress bar under the sheet header.
- **Empty states.** The first-run card (spec 79): 480 px wide, centred in
  the Viewport, `surface.raised`, 24-px padding, an 18-px heading, three
  primary-looking rows (Import a drawing, Import a picture, Open a project)
  as 40-px secondary buttons with icons, then *Examples* and three quiet
  rows; `x` top-right. Empty lists show one 13-px text.secondary sentence
  centred in the list area, never a graphic.
- **Destructive actions.** Delete, Remove and Duplicate never confirm; the
  toast names the undo. There are two confirmations. The first is unsaved
  changes on close, open or quit: a 420-px sheet with the file name,
  *Save*, *Discard* (red text on hover) and *Cancel*; Enter is Save, Escape
  is Cancel. The second is the Tool Library's *Remove* (#581): the library
  is outside project history, so a removal cannot be undone. The question
  replaces the table in place and names the ticked tools ("Remove 2 tools
  from your library: IDC 1/4 in Downcut, Amana 45771? Projects keep their
  copies."), with *Remove* (destructive) and *Cancel* in the footer;
  Escape is Cancel. Remove on a tool in use is refused inline: the button
  disables and its tooltip names the operations.
- **Sheets (import, export, Tool Library, unsaved changes).** A
  `surface.raised` panel centred over a `#00000066` scrim, 8-px radius,
  32-px header with title and `x`, the same sections, fields and
  validation as the panels, and a footer with the one primary action on
  the right and secondary actions on the left. Never a second sheet above
  a sheet: the tool picker from an editor is a popover, the Fusion import
  report replaces the Tool Library's table in place with a *Back*, and so
  does *Remove*'s question, with *Cancel*.
- **Keyboard focus.** Tab walks top-left to bottom-right: toolbar, Design
  Tree, Properties, Viewport, CAM list, editor, status bar; inside a sheet
  it cycles within the sheet. Every focusable element shows the 2-px ring
  (section 7). The ring shows for keyboard focus only, not after a mouse
  click (egui note in 12). Arrow keys move within lists; Enter activates;
  Space toggles; Delete deletes the selection; F1 opens the chapter.

## 9. The viewport

- **Background and grid.** `viewport.bg` fills the pane; the stock is a
  `viewport.stock.fill` rectangle with the dashed outline (1.5 px, dash
  6/4), so the stock reads as a sheet laid on a bench, not a frame on
  a canvas. Grid step `s` is the smallest of 1, 10, 100, 1000 mm (0.1, 1,
  10 in for inch projects) whose on-screen spacing is ≥ 12 px; minor lines
  every `s`, major every `10 s`. Minor lines fade in linearly from 0 at
  12 px to full at 28 px spacing so the grid thins rather than pops. The
  grid covers the stock and the male board only, never the whole pane;
  outside them the pane is plain. Axis lines through the origin are 1 px
  `viewport.axis`; the origin crosshair is 12 px with a 3-px gap at the
  centre and a `0,0` label (12 px) below-left.
- **Line weights, screen-constant (px at 1×, multiplied by scale).** Shape
  1.25; open path 1.25 dashed 4/3; hovered shape 1.25 in `shape.hover`;
  selected shape 2 in `shape.selected`; locked 1.25 in `shape.locked`;
  toolpath cut 1.5; rapid 1 dashed 4/3; plunge mark a 6-px circle outline;
  start circle the bit's diameter in mm but never under 8 px, tick one
  diameter long; tab mark 4 px wide and the tab's width long; drill hole
  the bit's diameter, never under 8 px; direction arrows 6-px chevrons
  every 40 px of screen length; extents centre box 1 px solid, edge box
  1 px dashed 6/4 with its caption (`X −6.35…156.35 Y −6.35…96.35`) in a
  `viewport.label.bg` pill at its top-left; male board outline 1.25
  dashed 6/4. Nothing scales with zoom except positions and the mm-sized
  marks; this is what keeps a 500-shape project from turning into a
  smear at fit.
- **Selection and hover order.** Hover draws under selection; selection
  draws over toolpaths; toolpaths draw over the picture; the picture over
  the grid. Selecting an operation (row or toolpath click) raises its
  toolpath to the top and draws a 4-px halo in its colour at 25 % alpha
  beneath it; other toolpaths stay at full alpha unless *Only the selected
  operation* is on.
- **Cursor coordinates.** The status bar's cell updates every frame in
  tabular 12 px: `X 64.2  Y 51.8` (one decimal in mm, three in inch),
  counting from the origin. No crosshair cursor and no floating readout:
  the pane stays clean.
- **Male inlay board.** Drawn to the right of the stock with a gap of
  20 % of the stock's width (minimum 20 mm), as `--preview` places it:
  its own `viewport.male-board` outline, a smaller origin mark, the
  targets mirrored in `viewport.male-board`, the band and cut-out
  toolpaths in the Inlay's colour. Its shapes are not selectable; hover
  shows `Male board · Inlay 1` in the status bar; a click selects the
  Inlay. A `Male board` caption (12 px, label pill) sits at its top-left.
  Fit includes it.
- **Visibility card.** A 200-px `surface.raised` card at the pane's
  top-right, 12-px inset, 8-px radius, 1-px `border.default`, shadow;
  header *Toolpaths* 13 px SemiBold with a collapse chevron; then the
  colour-mode segmented control, six 24-px checkbox rows, the *Only the
  selected operation* checkbox, and *Show all* / *Hide all* quiet buttons.
  Collapsed it is the header only. Its state persists in `gui.toml`.
- **Depth legend.** Shown only in By depth mode, left of the card: a
  12 × 160 px vertical bar of the ramp, ticks with 12-px labels at the
  stock top, each operation's depth and the deepest cut, in project units
  with the unit once at the top (`0 mm`, `−6`, `−12.2`). Labels that would
  overlap stack to the right.
- **Program extents.** The two boxes above; the caption names the edge box
  as the cutter's reach: *Program extents (bit edge, dashed)*. The status
  bar shows the Z range in the same tabular style.
- **Not 1998.** No black background by default, no bevelled frame around
  the pane, no scroll bars (pan and zoom instead), no rulers, no
  coordinate readout chasing the cursor, no shadow under the stock, no
  crosshair cursor, no 1-px aliased lines (every stroke is anti-aliased
  through epaint's feathering), no neon green on black.

## 10. Motion

- Disclosure sections, the visibility card's collapse, the Warnings drawer
  and panel collapse to a strip: 150 ms ease-out on height; content clips.
- Hover washes and divider highlight: 100 ms ease-out; focus ring and
  pressed state: instant.
- The Undo toast: 150 ms in, 150 ms out after 2 s; the row flash: 300 ms.
- The spinner: a 14-px arc, 270° of a circle, rotating once per 800 ms,
  linear; the only continuous animation in the app.
- Never animated: anything on the Viewport (selection, zoom, pan, toolpath
  replacement all happen in one frame), list reorder (the drop is
  instant), sheet open and close, field validation, colour-mode switch.
- Reduced motion (section 11): all durations become 0 except the spinner,
  which becomes a static three-dot glyph.

## 11. Accessibility

- Contrast: text ≥ 4.5:1 on its surface (every text token in section 5
  meets this in both modes, including the status text colours); glyphs,
  outlines, handles, toolpaths and marks ≥ 3:1 (all meet it except grid
  lines and the pale end of the depth ramp, which are intentionally
  faint and never carry information on their own).
- Focus is always visible: the 2-px ring at 5.2:1 (light) / 6.2:1 (dark)
  against panels, outside the control so no control shrinks.
- Every action has a keyboard path: menus list every shortcut (spec 54);
  lists take arrow keys; sheets trap Tab; Escape closes the innermost
  transient thing.
- Hit sizes: no interactive element smaller than 24 × 24 px (checkboxes
  and 16-px icons sit in 26-px hit rows; the eye and lock in a 24-px
  square). Dividers accept 8 px of pointer slop either side.
- Colour is never the only signal: errors have a filled glyph and a
  message; warnings a triangle; operations have their glyph and name
  beside their colour; selection has handles and a bar, not only a tint;
  locked shapes carry the padlock; open paths carry the `open` badge.
  The eight operation colours were chosen so adjacent pairs differ in
  lightness as well as hue; a future per-operation colour override is out
  of scope but the token table makes it cheap.
- Reduced motion follows the OS setting where the platform exposes it;
  otherwise a `gui.toml` key. Zoom level of the UI follows the OS scale
  factor; the type scale is not user-adjustable in this phase.

## 12. egui realisation notes

Written against egui 0.31; names that changed at 0.31 are given as
`corner_radius` (formerly `rounding`). Set everything once in a
`theme::apply(ctx, Appearance)` call at startup and on an OS appearance
change; no widget sets a colour literal (the one rule a reviewer greps
for: `Color32::from_rgb` outside `theme.rs` and the painter-glyph module
is a bug).

- **Following the OS.** `ctx.options_mut(|o| o.theme_preference =
  ThemePreference::System)`; egui reports `ctx.theme()` from the window
  backend (eframe/winit). Build both `Visuals` and install with
  `ctx.set_visuals_of(Theme::Light, light)` and
  `ctx.set_visuals_of(Theme::Dark, dark)`.
- **Fonts.** `FontDefinitions::default()` minus egui's built-ins; insert
  `Inter-Regular`, `Inter-Medium`, `Inter-SemiBold`, `JetBrainsMono-Regular`,
  `lucide` as `FontData::from_static`. Families: `Proportional` →
  `[Inter-Regular]`, `Monospace` → `[JetBrainsMono-Regular]`,
  `Name("medium")` → `[Inter-Medium]`, `Name("semibold")` →
  `[Inter-SemiBold]`, `Name("icons")` → `[lucide]`. Weight is a family,
  not a property, which is why the three static files are bundled. Text
  styles: `Small` 12/Proportional, `Body` 13/Proportional, `Button`
  13/Proportional, `Heading` 15/semibold, `Monospace` 12/Monospace,
  `Name("Title")` 13/semibold, `Name("Medium")` 13/medium,
  `Name("Display")` 18/semibold. Line height: `FontId` has none; set
  `Style::spacing.item_spacing.y = 4` and let widgets take their height
  from the font plus `button_padding`.
- **Tabular figures.** egui (epaint + ab_glyph) does no OpenType feature
  selection, so the bundled Inter is feature-frozen with `tnum`. No
  ligatures are applied either, which is right for G-code in the mono
  face. Kerning is applied by epaint.
- **Spacing.** `item_spacing (8, 4)`, `button_padding (8, 4)` (gives 26-px
  controls with the 13-px font plus the 1-px stroke), `window_margin 12`,
  `menu_margin 4`, `indent 16`, `interact_size (24, 26)`, `icon_width 16`,
  `icon_width_inner 10`, `icon_spacing 8`, `combo_width 160`,
  `text_edit_width 160`, `tooltip_width 320`, `scroll: ScrollStyle::thin()`
  with `bar_width 6`, `floating true`, `dormant_background_opacity 0`,
  `active_background_opacity 0.4`, `interact_background_opacity 0.6` (the
  overlay scrollbar; egui's is good enough, no custom one).
- **Visuals.** `dark_mode`, `window_fill` = surface.raised, `panel_fill` =
  surface.panel, `extreme_bg_color` = surface.sunken, `faint_bg_color` =
  surface.stripe, `window_stroke` = `(1, border.default)`,
  `window_corner_radius 8`, `menu_corner_radius 6`, `window_shadow` =
  `Shadow { offset (0, 4), blur 16, spread 0, color #0000001F light /
  #0000004D dark }`, `popup_shadow` the same at blur 12,
  `selection.bg_fill` = accent.soft, `selection.stroke` = `(1, accent)`,
  `hyperlink_color` = accent, `error_fg_color` = status.error text,
  `warn_fg_color` = status.warning text, `striped true`,
  `button_frame true`, `collapsing_header_frame false`,
  `indent_has_left_vline false`, `slider_trailing_fill true`,
  `handle_shape Rect { aspect_ratio 0.5 }`, `text_cursor.stroke` =
  `(1.5, accent)`, `interact_cursor Some(PointingHand)` for buttons only.
  `widgets.noninteractive` = `{ bg_fill panel, weak_bg_fill panel,
  bg_stroke (1, border.default), fg_stroke (1, text.primary),
  corner_radius 4, expansion 0 }`; `inactive` = `{ bg_fill sunken,
  weak_bg_fill transparent, bg_stroke (1, border.strong), fg_stroke
  text.primary }`; `hovered` = `{ bg_fill sunken, weak_bg_fill
  surface.hover, bg_stroke (1, text.secondary), fg_stroke text.primary,
  expansion 0 }`; `active` = `{ weak_bg_fill surface.active, bg_stroke
  (1, accent) }`; `open` = `{ bg_fill raised, bg_stroke (1,
  border.strong) }`. Set `expansion 0` everywhere so hover never grows a
  control. Disabled text: egui derives it by fading `fg_stroke`; override
  with `text.disabled` via `Visuals::gray_out` → set
  `widgets.noninteractive.fg_stroke` and use `ui.add_enabled` so egui
  picks it.
- **Focus ring.** `Style::interact` returns the `active` style for a
  focused widget, which would paint the pressed wash; keep `active`'s wash
  light and draw the ring separately: after any `Response`, `if
  response.has_focus() && ctx.memory(|m| m.focused_via_keyboard())`
  (0.31 exposes whether focus came from Tab; if the running version does
  not, keep a frame-level flag set on Tab and cleared on click) then
  `painter.rect_stroke(rect.expand(3.0), 6.0, (2.0, focus.ring))`. One
  helper, `focus_ring(ui, &response)`, called by every custom widget and
  wrapped around egui's built-ins in `widgets::button` and
  `widgets::field`.
- **Custom widgets** (one module each under `gui/widgets/`):
  `unit_field` (TextEdit with a trailing unit label and the reset button
  drawn inside the same frame, parse on `lost_focus`),
  `segmented` (a `Frame` with `n` selectable labels sharing one stroke),
  `section_header` (`CollapsingState::show_header` with a custom header
  closure for the chevron, the title style and the trailing status
  glyphs), `tree_row`, `list_row` (grip, checkbox, glyph, two-line label,
  eye, status cell, with `dnd_drag_source` / `dnd_drop_zone` for reorder),
  `status_glyph`, `toggle` (egui has no switch; 28 × 16 painter widget),
  `toast`, `spinner` (egui's `Spinner` with `size 14` and the accent
  colour is enough), `op_glyph` (painter-drawn operation-kind icons).
  Tables use `egui_extras::TableBuilder` (MIT/Apache-2.0) with
  `striped(true)`, `resizable(true)`, row height 28.
- **Dropdowns.** `ComboBox` with `width` set and `icon` replaced by the
  Lucide chevron via `ComboBox::icon`; rows are `selectable_value` with a
  leading `check` glyph drawn by a custom row closure. The tool dropdown
  is a custom popup (`Popup`/`Area` under the field) because its rows have
  glyphs and a trailing action.
- **Popovers.** egui's `Popup`/`Area` floats above everything in the same
  viewport, so popovers and the visibility card are real; the limit is
  that they cannot leave the window (no OS-level popups). Menus come from
  `egui::menu` with the `menu_margin` above; their shortcut hints are a
  right-aligned `Small` label in text.secondary.
- **Text truncation.** `Label::new(..).truncate()` (`TextWrapMode::Truncate`)
  gives an ellipsis; use it on every row name and never wrap in a row.
  Middle truncation (`plaque…nc`) does not exist; file names truncate at
  the end and the tooltip shows the whole.
- **Tooltips.** `response.on_hover_text` with `interaction.tooltip_delay
  0.5` and `tooltip_grace_time 0.2`; the field help tooltip is a
  `on_hover_ui` with two labels (Body, then Monospace).
- **Animation.** `style.animation_time 0.15`; disclosure uses
  `CollapsingState`'s built-in `animate_bool`; washes use
  `ctx.animate_value_with_time(id, target, 0.1)`. Reduced motion sets
  `animation_time 0` and skips `animate_value` by returning the target.
- **The viewport.** A `wgpu` callback (`egui_wgpu::Callback`) inside a
  `ui.allocate_rect(.., Sense::click_and_drag())`; the overlay (handles,
  labels, caption pills, the marquee, the legend) is drawn with egui's
  `Painter` on top so it uses the same fonts and tokens. Line widths are
  in points and multiplied by `pixels_per_point` in the pass; dashes are
  computed in screen space so they do not scale with zoom. The custom
  pass gets the token set as a uniform buffer rebuilt on theme change.
- **What egui cannot do, and the workaround.** OS-native menus (egui
  menus; revisit for macOS later); OpenType features (frozen font);
  letter-spacing in text styles (not used); font weights by property
  (one family per weight); variable fonts (static instances); subpixel
  text positioning (egui snaps glyphs to pixels: fine at 13 px, and the
  reason nothing is 11 px); italic (not bundled); native file dialogs
  (use `rfd`, MIT, for open/save/import/export paths only; the sheets
  are egui's); drag-and-drop of files from the OS (eframe gives
  `raw_input.dropped_files`: enough for `.dcv`, `.svg`, `.dxf`, images);
  accessibility tree (eframe's `accesskit` feature on; every custom widget
  calls `response.widget_info`); window title unsaved marker
  (`ViewportCommand::Title`).

## 13. Review checklist

A reviewer checks a PR's screenshots (light and dark, 1× and 2×) against:

1. Every surface colour is one of section 5's tokens; no `Color32` literal
   outside `theme.rs` and the painter-glyph module.
2. Both appearances were captured; nothing is white-on-white or
   dark-on-dark in either.
3. Control heights are 26 px (28 in sheet footers); row heights are 26,
   40 or 28 as the table says.
4. All horizontal and vertical gaps are on the 4-pt scale.
5. Text is Inter at 12, 13, 15 or 18 px, or JetBrains Mono at 12; no other
   size, no system font.
6. Every number column aligns (tabular figures); units follow the value
   inside the field in text.secondary.
7. Every field in an error or warning state shows an outline *and* a
   glyph *and* a message; no colour-only state.
8. Status red appears only with a glyph on chrome; operation 2's red
   appears only as a line on the Viewport.
9. No OK/Cancel/Apply row anywhere outside the unsaved-changes sheet;
   panels apply changes as they are made.
10. No sheet opens above a sheet; the tool picker is a popover.
11. Icons are Lucide at 16 or 20 px, or the operation glyphs; none at 24
    or 12 in the chrome.
12. Section headers are 28 px with the chevron on the left and a located
    warning's glyph on the right when closed.
13. The focus ring is visible on the keyboard-focused control in the
    screenshot taken after Tab, and absent after a click.
14. Disabled controls show their reason on hover where the spec names one.
15. The Viewport's lines are anti-aliased and screen-constant: a fit view
    and a 4× zoom show the same stroke widths.
16. The stock outline is dashed and the stock fill is one shade off the
    background; the grid stops at the stock's edge.
17. Selected shapes show the accent box, four corner handles and the
    rotation handle; locked ones show grey handles and a padlock.
18. Stale toolpaths are at 35 % alpha with the row's spinner showing.
19. The visibility card is a floating raised card, collapsible, with the
    depth legend beside it in By depth mode.
20. The Warnings drawer opens from the status bar counts and groups
    project-level rows first, each row with tier glyph, mono code, message.
21. The empty state is a centred raised card with three buttons and three
    example rows, and the empty lists show one sentence each.
22. Nothing animates on the Viewport; a disclosure animates in 150 ms.
23. Every string in the screenshot is in the string table (spot-check two).
24. The window at 1280 × 768 shows both side panels at their minimum with
    the Viewport at least 560 px wide.

## 14. Anti-patterns

| Dated | Replace with |
| --- | --- |
| Bevelled, gradient or glossy buttons | Flat fill or outline, one wash on hover, 4-px radius |
| OK / Cancel / Apply rows under a property page | Edit in place; Undo from Edit; the one sheet with buttons is unsaved changes |
| A modal over a modal | A popover inside the sheet, or the sheet's content replaced with a *Back* |
| Tabbed property sheets (General / Advanced / Tabs…) | Disclosure sections in one scrolling editor |
| A "?" button beside every field | Help on label hover, with the field name in mono |
| 16-px pixel icons on a grey toolbar strip | 20-px Lucide icons with labels, quiet buttons, one primary |
| Status bar text at 11 px in sunken cells | 12 px on a flat bar with 1-px rules |
| Raised 3-D frames around panels and the canvas | No frames; 4-px gutters and one rule under headers |
| Grid over the whole canvas, black background, crosshair cursor | Grid on the stock only, paper-coloured stage, arrow cursor, coordinates in the status bar |
| Lines that scale with zoom until the picture is a blob | Screen-constant stroke widths |
| Red text as the only sign of an error | Outline + filled glyph + message |
| Radio-button groups for Climb / Conventional | A segmented control |
| A spinner dialog during generation | Spinners in rows, faded stale toolpaths, *Generating 2 of 5…* in the status bar |
| Confirm dialogs on Delete | Delete, toast, Undo |
| Dark mode by inverting the light palette | Two token columns designed together; the operation palette checked on both backgrounds |
| Nested tree indentation with dotted connector lines | Flat rows under three section headers; no connectors |
| A splash screen | The empty-state card in the window |
