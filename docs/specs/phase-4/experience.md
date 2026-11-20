# Phase 4: Experience and visual design language

Status: draft, written 2026-11-02 for the phase 4 design spec; the design
spec's "Visual design" section points here. Amended 2026-11-10 by the
screenshot review of #549 (functional spec, "Review redesign").
Amended 2026-11-18 by the redesign (`redesign.md`): the principles of
section 1, the window of section 2, the sizes of section 3, the tokens
of section 5, the controls of section 7 and their egui realisation in
section 12 are the redesign's. Amended 2026-11-19 by the redesign's
review (slice D5): section 13 is the checklist for the design as built,
ticked against the pictures in `review/final/`, and what the first
review added (it was section 15) is folded into the sections it
belongs to. It
designs how the app in `functional.md` looks and behaves, inside that
spec's capabilities (50–80); it adds no feature and renames none. The
wireframes in `mockups/` show the window before the redesign and are
kept for their labels and field names; this document and `redesign.md`
fix the layout and everything the wireframes leave grey.
Vocabulary is `CONTEXT.md`'s. Realised in egui + wgpu; the realisation notes
(section 12) say how, and what egui cannot do.

## 1. Principles

1. **The app says what to do next.** The work is five steps in a fixed
   order: Design, Stock, Tools, Operations, Export. The rail shows which
   are done, which need attention and which is current; one accent
   button in the header moves to the next step that needs something.
   Nothing is locked, and every panel edits the project in place: a
   change applies as it is made and is undone from Edit. *Rules out:* a
   wizard that blocks, a blank window with a menu bar, a toolbar of
   twelve equal buttons, nested modals, tabbed property sheets,
   OK/Cancel/Apply rows, a "Properties" window that floats apart from
   the thing it edits.
2. **Numbers are the interface.** Every length and feed is a value, a unit
   and a validation state, aligned so columns of them read as a table.
   *Rules out:* proportional figures in value fields, units in the label
   instead of after the value, bare numbers whose unit the user must infer,
   red text as the only sign that a field blocks export.
3. **One severity ladder, everywhere.** Error (export refuses) is red with a
   filled glyph; warning is amber with an outlined glyph; hint is blue-grey
   with a dot. The ladder reads the same in a field, a row, the status bar
   and the Warnings drawer. *Rules out:* a second red (operation colour 2 is
   red and must never be mistaken for an error), coloured text without a
   glyph, warnings that look like hints.
4. **The Viewport is the picture of what the machine will do.** It draws
   what `--preview` draws, at screen-constant line weights, on a calm
   background that lets eight operation colours stay distinct. *Rules out:*
   decorated grids, drop shadows on geometry, 3D-looking stock, chart-like
   gradients, anything the CLI's SVG would not show.
5. **Roomy, still a tool.** 13-px text, 32-px controls, cards 8 px
   apart with 16 px of padding. A panel shows one job at a time, so
   nothing needs to be small. *Rules out:* 26-px controls, text under
   12 px (the rail's 11-px label under its icon is the one exception), a
   second column of fields squeezed into a panel, padding that varies by
   panel.
6. **Dark first; light is the same design.** Every token has two values
   chosen together and contrast-checked; the operation palette is
   checked on both backgrounds. One accent marks the current step, the
   selected row, focus and the one primary button on screen. *Rules
   out:* a light mode made by inverting, a white Viewport in dark mode,
   colours that pass in one mode only, two accent buttons in one view,
   accent on a toolpath.

## 2. The workspace

The window is six regions (functional 50; `redesign.md`, "The window"):
a header, a rail, the step panel, the Viewport, the Inspector and a
status bar. Chrome is quiet so the stock and its toolpaths are the
brightest thing on screen, and one accent says where the user is and
what to press next.

- **Surfaces.** Every region but the header and the status bar is a
  **card** on the window backdrop (`surface.window`): `surface.panel`, a
  14-px radius, no border and no shadow. The backdrop shows as the 8-px
  gaps between the cards and the 8-px margin round them, and under the
  header and the status bar, which sit on it. The cards are the rail,
  the step panel, the Viewport, the Inspector and the Warnings drawer.
  The Viewport's card is `viewport.bg`: darker than the cards in dark
  mode, a shade off white in light, so it reads as the stage. Popovers,
  menus, the toolpath view popover, the Viewport's floating tools and
  sheets are `surface.raised` with the one soft shadow (popovers and
  menus also a 1-px `border.default`, since in light mode they are the
  card's own white). A card inside a card (the Export step's readiness
  card, a start-screen example) is `surface.raised` with a 10-px radius
  and a hairline. Fields and lists are `surface.sunken`. Dark is the
  default appearance; light is the same design.
- **Header.** 48 px on the backdrop, across the window. At the left the
  menus (File, Edit, Arrange, View, Project, Help), quiet text buttons
  (egui's own menu bar; on macOS the design spec may move it to the
  native bar later: nothing here depends on it). In the centre the
  project's name, 13-px Medium, with a 6-px text.secondary dot before it
  while there are unsaved changes; it is centred on the window while
  the room between the menus and the right-hand group allows, and cut
  with an ellipsis when it does not. The same name is the OS window
  title (`plaque — DreamCarve`, `plaque* — DreamCarve` when unsaved), set
  with `ViewportCommand::Title`. At the right: Undo and Redo, 32-px
  quiet icon buttons whose tooltips name the step; the generation chip
  while toolpaths regenerate (a 24-px pill: the spinner and *Generating
  2 of 5…*, or *Stopped*; *Stop* and *Regenerate all* stay in the status
  bar); and the one primary button, a 34-px accent pill. It reads
  *Continue* with `arrow-right` after the label and opens the next step
  that is not complete. On the Export step the header has no primary
  button: the step's *Export G-code* is at the foot of its panel
  (section 7, "Export foot").
- **Rail.** A 64-px card at the left, full height. Five step buttons
  from the top, 4 px apart, each 56 × 52 with a 10-px radius: a 24-px
  icon over an 11-px label (Design `shapes`, Stock `box`, Tools `drill`,
  Operations `route`, Export `file-output`). At rest icon and label are
  text.secondary; under the pointer text.primary on `surface.hover`; the
  current step is filled with `accent.soft` and its icon and label take
  `accent`. The state mark sits at the icon's top right on a disc of the
  surface under it: nothing while the step is empty, a 12-px `check` in
  status.success when complete, an 8-px dot in status.warning or
  status.error when it needs attention; the tooltip says it in words
  (*Tools: has an error*) with the step's shortcut. Pinned to the
  bottom, three 40-px quiet icon buttons: Warnings (`triangle-alert`,
  with a 16-px badge counting the worst tier found, in that tier's
  colour; pressed while the drawer is open), Appearance (`sun-moon`, a
  menu of System, Light and Dark with the current one checked) and Help
  (`circle-question-mark`, a menu of the Help menu's rows). The steps
  are tabs to assistive tools, the current one selected.
- **Step panel.** A card, 312 px wide by default and at least 280. Its
  head, 16 px in: the step's name in 18-px SemiBold, and under it one
  12-px text.secondary line saying what the step holds (*10 shapes*,
  *170 × 120 × 11.8 mm*, *1 error blocks export.*) or, while it holds
  nothing, what to do in it. Then the step's content, to the card's
  foot:
  - **Design**: *Add drawing* and *Add picture*, two equal secondary
    buttons, over the Design Tree (Shapes, Tabs, Pictures).
  - **Stock**: Units, the stock's width, height and thickness with
    *Fit to shapes*, and the Origin section. The stock's own fields have
    no section header: the step's title names them.
  - **Tools**: *New tool* and *Add from library…*, the list, *Duplicate*
    and *Delete* under it, and a quiet *Tool Library…* at the card's
    foot.
  - **Operations**: *Add*, *Duplicate*, *Delete*, then the list in cut
    order.
  - **Export**: the whole export, top to bottom (functional 77), in a
    panel 440 px wide: the step has no Inspector, and a program's name,
    tool and time read on one line. First the readiness card (the error,
    warning and hint counts as status chips, or `circle-check` and *No
    problems*; *Estimated time* as the 22-px summary number; *Z range,
    tool centre* once the programs are built). The card counts what
    `check` reports, so it agrees with the card under it: on a blank
    project it reads *1 error*, the unset thickness, where the status
    bar still says *No problems* (functional 50b). While an error blocks
    export, a card in the status.error tint under it: `circle-x` and *1
    error blocks export* in 13-px Medium with a quiet *Show* at its
    right, then each finding as a row, its message wrapped over its
    field in the mono face, the row a button with `surface.hover` under
    the pointer. Then the Post-processor, Heights and Tolerances
    sections, closed until wanted, each with one 12-px text.secondary
    line of its values after its title (*grblHAL v4 (latest) · .nc*).
    Then the plan: *Programs*, a full-width dropdown under its label
    since its choices are sentences, where the project cuts with several
    tools; the board ticks with an Inlay; *Program Details*, a
    disclosure that says how many programs it holds after its title
    while closed, as the three sections above it say their values, and
    open shows striped rows, each a program's number in a gutter
    beside three lines (its file name in the mono face with its time and
    Z range at the right; its tool and operations; the step before it in
    12-px text.secondary); and *Destination* (the folder in the mono
    face, cut at its start, with *Choose…*) over *File name*. Once the
    programs are written the result takes the plan's place (section 7,
    "Export result").

  While an operation's targets are edited (section 8) the panel shows
  the Design Tree, the one part of the window besides the Viewport that
  stays live.
- **Inspector.** A card at the right, 328 px wide by default and at
  least 300, there only while the current step has something selected
  that has an editor. Its 48-px header has a quiet `x` at its right
  that deselects; with nothing selected there is no card and the
  Viewport takes the room. On Design: *Properties · square* in 13-px
  SemiBold, the Arrange group (section 7, "Split button") before the
  close button, and the placement fields. On Tools: the tool's type
  glyph and name, its fields, and *Save to library* in a footer that
  stays in view. On Operations: the operation editor with its own
  header (section 7, "Editor header").
- **Hierarchy.** Three levels only: step title (18 px), section header
  (Shapes, Origin, Setup, Depth, Strategy), row. No fourth. An
  Inspector's header sits at the section-header size, 13-px SemiBold;
  section headers are 13-px SemiBold with a chevron and no band; rows
  are 13-px Regular.
- **Required first.** The rule behind functional 66 and 66a: a panel
  that edits a record leads with the few values that make it right,
  then folds the rest under headers that say what they hold. A user who
  never opens a section has still seen every value, as a summary. The
  same rule shapes the Stock step (Units, then the stock's size), the
  tool editors (name, type, diameter, the three rates first) and the
  Export step (its three settings sections closed, each with its
  values after its title).
- **Dividers.** No card has a border, a title band or a rule under its
  title. The 8-px gap at the inner edge of the step panel and of the
  Inspector is the divider: it drags, the cursor is the platform's
  resize cursor, and a 2-px `accent` line shows in it while the pointer
  is on it. The widths are remembered. View ▸ *Step Panel* hides the
  step panel, and choosing a step shows it again.
- **Viewport.** Its controls float over it (section 9, "Floating
  controls"): the tools, *Fit* and the zoom in one 36-px
  `surface.raised` pill at its top left, the colour mode and *Layers* in
  another at its top right, and the cursor's read-out in a 28-px pill at
  its bottom left, each with a hairline and the one shadow, 12 px in
  from the card's edges. While the project has nothing to cut and the
  Design step is open, the card shows the start screen (section 8,
  "Empty states") in place of the stock, and no control floats.
- **Status bar.** 28 px on the backdrop, with a 6-px margin under it;
  each cell a quiet 24-px pill (`surface.panel`, a 1-px `border.default`
  outline). The cells are in two groups (functional 50b), so that none
  moves when another comes or goes. From the left, what holds still:
  units (a quiet button, `mm` with a `chevron-down`, opening a two-row
  menu); counts (errors, warnings, hints, each a glyph + count in its
  status colour; one button that opens the Warnings drawer;
  `circle-check` + *No problems* in text.secondary when all are zero);
  then the message, which takes the room that is left and truncates at
  the end. From the right, what comes and goes: one cell for the job
  (while toolpaths generate, the spinner + *Generating 2 of 5…* +
  *Stop*, or *Stopped* + *Regenerate all*; when they are current, the
  estimated time), and left of it one cell for the pointer (a drag's
  travel, else the name of the shape under it, in text.primary). A cell
  with nothing to show is not drawn. Cells are 6 px apart with 10-px
  padding and no rules between them; cell text is 12 px. A cell that is
  a button takes `surface.hover` on its pill.
  No cursor, no Z range and no Toolpaths button: they float on the
  Viewport. No stock size: the Stock step's summary line shows it.
- **Warnings drawer.** A card over the bottom of the Viewport's card,
  240 px high and 8 px in from its edges, `surface.panel` with a
  hairline and the one shadow; never a window. The Viewport keeps its
  size under it, and its read-out and Fit stand clear of it. A grab
  handle, a 36 × 4 px pill in `border.strong` centred at its top
  (`accent` under the pointer), drags it taller or shorter, from 120 px
  to where the Viewport's top controls would be covered. It opens from
  the rail's Warnings button or the status bar's counts.
- **Small windows.** 1280 × 768 holds the rail, the step panel (280),
  the Viewport (520) and the Inspector (300) with their five 8-px gaps.
  In a narrower window the Inspector lies over the Viewport's right
  side, a card with a hairline and the one shadow, instead of beside it.
- **Persistent vs transient.** Persistent: the header, the rail, the
  step panel, the Viewport and its floating controls, the status bar,
  the depth legend while By depth is on (section 9), the target-editing
  bar while target editing is on (section 8). The Inspector comes and
  goes with the selection. Transient: menus, the Add-operation menu, the
  tool picker popover, the **toolpath view popover** (opened from the
  Viewport's *Layers*), the Rotate popover, tooltips, the help line, the
  marquee, insertion indicators, toasts for Undo. Sheets (Import
  drawing, Add picture, Tool Library, unsaved-changes) are transient and
  modal; everything else is not. Export is a step, not a sheet.
- **A sixth step.** Open question 3 asks that another panel fit without
  redesign: the rail is the seam. A step is a button on the rail, a
  content for the step panel and, when it has an editor, a content for
  the Inspector; nothing in this document assumes five.

## 3. Layout grid and spacing

A 4-pt scale. Everything snaps to it; the only 2-px uses are hairline
gaps, and the one off-scale step is the 6-px margin under the cards and
the status bar, which with a pill's 2-px inset makes the 8-px gap.
(`redesign.md`, "Shape, size and type", is where these numbers come
from.)

| Step | px | Used for |
| --- | --- | --- |
| 0.5 | 2 | Gap between a glyph and its count; focus-ring offset; gap between a row's two lines |
| 1 | 4 | Gap between icon and label inside a row, vertical gap between stacked controls |
| 2 | 8 | Gap between cards and the margin round them, gap between a label and its field, a list row's inset from its card's edge and its own padding, gap between adjacent buttons |
| 2.5 | 10 | Field and chip inner padding (horizontal), status-bar cell padding |
| 3 | 12 | Secondary-button padding (horizontal), popover margin |
| 4 | 16 | Card padding, primary-button padding (horizontal), gap between sections inside a sheet |
| 5 | 20 | Gap between sections in a card: under an open section's body |
| 6 | 24 | Sheet padding, the start screen's margin |
| 8 | 32 | Control and header heights, the gap between the start screen's blocks |

Corner radii:

| Thing | Radius |
| --- | --- |
| Card | 14 |
| Sheet | 16 |
| Menu, popover | 12 |
| Card inside a card (example card, clearing pass, callout, import preview) | 10 |
| Control (field, dropdown, secondary and quiet button, list row, menu row) | 8 |
| Checkbox, thumbnail | 4 |
| Pill (primary button, pill tab, segmented control, chip, status-bar cell, switch, Viewport caption) | Half its height |

Heights (all at 1× scale; egui multiplies by `pixels_per_point`):

| Element | Height | Notes |
| --- | --- | --- |
| Header | 48 | On the backdrop; buttons 32 tall inside, the primary one 34 |
| Rail button | 52 | 56 wide in the 64-px rail; its bottom buttons 40 square |
| Step title | 24 | 18-px SemiBold, 14 px under the card's top; its 12-px summary line under it |
| Inspector header, editor header | 48 | No band and no rule |
| Warnings drawer's header | 44 | With its buttons, under a 14-px grab strip |
| Target-editing bar | 48 | Its two buttons are controls, 8 px in |
| Section header | 32 | Full-width hit area, no band |
| Control (field, dropdown, button, segmented, pill tabs) | 32 | Text baseline centred |
| Primary button in the header | 34 | The one taller control |
| Checkbox glyph | 16 | Inside a 32-px row |
| Switch | 20 | 36 wide, inside a 32-px row |
| Chip | 24 | 12-px text |
| Design Tree row | 30 | One line, on its own 28-px surface |
| Operations and Tools list row | 52 | Two lines: name, then tool or size; on its own 50-px surface |
| Table row (Tool Library, export programs, Warnings) | 34 | One line |
| Menu row, dropdown row | 32 | |
| Status bar | 28 | Cells 24, and a 6-px margin under it |
| Toolpath popover row | 24 | The key; its checkbox rows are 32 |
| Floating pills (Viewport) | 36 | Controls 28 inside; the read-out's pill is 28 |
| Vendor group header (Tool Library) | 32 | Same as a section header, inside the table |
| Example card (start screen) | 144 | A 72-px well with the 96 × 56 thumbnail, then the name and one line |
| Toolpath view popover | auto | 240 wide |

Widths are the cards': the rail 64; the step panel default 312, min 280;
the Inspector default 328, min 300; the Viewport min 520 (so 1280 × 768
holds all four at minimum with the five 8-px gaps, spec 50). Labels in
editors take a 112-px column, in every editor, and a label is never
cut: one that does not fit on a line wraps at its words to two, which
is a control's height, so the rows stay aligned (*Through-cut
allowance*, *Tool-change machine Z*). The field fills the rest;
the unit occupies the last 28 px inside the field. Tool-Library sheet
1100 × 640 default (its table's ten columns, a type's name whole among
them, leave the name 120 px), resized by the user between 920 × 480
and the window less 24 px all round, its right (editing) pane
320 wide;
Import drawing and Add picture sheets 560 wide; unsaved-changes sheet
420 wide; the Export step's panel 440 wide; the start screen's content
624 wide.

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
| Step title, start-screen heading, sheet title | Inter | 18 | SemiBold | text.primary | `Name("Display")` |
| Summary number (the Export step's estimate) | Inter | 22 | SemiBold | text.primary | `Name("Summary")` |
| Rail label | Inter | 11 | Regular | as its icon | `Name("RailLabel")` |
| Heading inside a sheet | Inter | 15 | SemiBold | text.primary | `Heading` |
| Inspector title, drawer title | Inter | 13 | SemiBold | text.primary | `Name("Title")` |
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
sets (section 12). The values are the redesign's (`redesign.md`,
"Tokens"); dark is the default appearance.

### Surfaces, borders, text, accent

| Token | Light | Dark | Visuals field |
| --- | --- | --- | --- |
| surface.window | `#ECEFF1` | `#0C0F11` | the backdrop: the fill of every panel's own frame, and the clear colour |
| surface.panel | `#FFFFFF` | `#14181B` | `panel_fill` (cards) |
| surface.raised | `#FFFFFF` | `#1B2024` | `window_fill` (menus, popovers, sheets, a card inside a card), `widgets.open.bg_fill` |
| surface.sunken | `#F3F5F6` | `#0F1315` | `extreme_bg_color` (text edits, lists), `widgets.inactive.weak_bg_fill` (dropdowns) |
| surface.stripe | `#F7F8F9` | `#181D20` | `faint_bg_color` |
| surface.hover | `#141A1D10` (6 % of text.primary) | `#E8EDEF10` | `widgets.hovered.weak_bg_fill` |
| surface.active | `#141A1D1A` (10 %) | `#E8EDEF1A` | `widgets.active.weak_bg_fill` |
| border.default | `#DCE2E5` | `#252B30` | `widgets.noninteractive.bg_stroke`, `widgets.inactive.bg_stroke` (field outline at rest), `window_stroke`; rules inside a card |
| border.strong | `#B8C2C8` | `#3A434A` | `widgets.hovered.bg_stroke` (field outline on hover) |
| text.primary | `#141A1D` (17.6:1 on panel) | `#E8EDEF` (15.1:1) | `widgets.*.fg_stroke.color`, `override_text_color` unset |
| text.secondary | `#52606A` (6.5:1) | `#9AA6AD` (7.2:1) | `widgets.noninteractive.fg_stroke` for labels (set per widget) |
| text.disabled | `#97A2A9` | `#66727A` | `weak_text_color` (placeholders), `widgets.noninteractive` when `!enabled` |
| text.on-accent | `#FFFFFF` (5.4:1 on accent.fill) | `#04261B` (7.4:1) | set per widget; dark text on the bright fill in dark mode |
| accent | `#08795E` (5.4:1 on panel) | `#2AD79A` (9.6:1) | `hyperlink_color`, `selection.stroke`; icons, links, the selected row's bar |
| accent.fill | `#08795E` | `#22C78C` | primary button fill, a checked box, a switch that is on |
| accent.hover | `#0A8A6C` (4.3:1 with white text) | `#3FE0A8` (9.6:1 with text.on-accent) | primary button on hover |
| accent.soft | `#08795E1F` | `#2AD79A29` | `selection.bg_fill` (selected rows, text selection) |
| focus.ring | `#08795E` | `#2AD79A` | custom (12); `widgets.active.bg_stroke` (a focused field's outline) |

The accent is a green/teal (hue 159°), and operation colours 3 (green)
and 6 (cyan) moved away from its hue so no toolpath reads as chrome.

### Status

Each status colour has a text value (meets 4.5:1 on panel in its mode), a
stroke value for glyphs and field outlines (≥3:1), and a tint for row and
field backgrounds. Never used without a glyph or a word.

| Token | Light text / stroke / tint | Dark text / stroke / tint | Glyph (Lucide) | Visuals |
| --- | --- | --- | --- | --- |
| status.error | `#C62E27` / `#C62E27` / `#C62E2714` | `#F2726A` / `#F2726A` / `#F2726A1F` | `circle-x` filled | `error_fg_color` |
| status.warning | `#A3580A` / `#D98A1E` / `#D98A1E1A` | `#F0B347` / `#F0B347` / `#F0B3471F` | `triangle-alert` outline | `warn_fg_color` |
| status.info (hint) | `#2A6CC4` / `#2A6CC4` / `#2A6CC414` | `#78B0F5` / `#78B0F5` / `#78B0F51F` | `info` outline, or a 6-px dot in a count | custom |
| status.success | `#1F7F43` / `#1F7F43` / none | `#5FD08A` / `#5FD08A` / none | `circle-check` | custom |

The light warning *text* is darker than its stroke so the message under a
field passes 4.5:1 while the outline and triangle keep the amber the eye
expects (`#D98A1E` is 2.8:1 on panel, so it is never used for text). The
dark success green is yellower than the accent (`#5FD08A` against
`#2AD79A`), so *done* does not read as *selected*.

### Viewport

The Viewport's card is the stage: darker than the cards in dark mode, a
shade off white in light.

| Token | Light | Dark | Contrast on viewport.bg |
| --- | --- | --- | --- |
| viewport.bg | `#F6F7F7` | `#0F1214` | — |
| viewport.grid.minor | `#ECEFF0` | `#15191C` | 1.1:1 (deliberately faint) |
| viewport.grid.major | `#DDE2E5` | `#1D2226` | 1.2:1 |
| viewport.axis (origin crosshair, X/Y axis lines through it) | `#6B7780` | `#9AA6AD` | 4.3 / 7.5 |
| viewport.stock (outline, dashed) | `#6B7780` | `#9AA6AD` | 4.3 / 7.5 |
| viewport.stock.fill | `#FFFFFF` | `#161B1E` | the stock is a shade off the background so its extent reads without the dashes |
| viewport.male-board (outline, origin mark, mirrored shapes) | `#8A929E` | `#7B8491` | 2.9 / 5.0 |
| viewport.shape (closed, solid) | `#1E2529` | `#E6EAEC` | 14.5 / 15.5 |
| viewport.shape.open (dashed, same colour) | as shape | as shape | |
| viewport.shape.hover | `#0A8A6C` (accent.hover) | `#3FE0A8` (accent.hover) | 4.0 / 11.1 |
| viewport.shape.selected | `#08795E` (accent) | `#2AD79A` (accent) | 5.0 / 10.1 |
| viewport.shape.locked | `#8A929E` + padlock glyph | `#7B8491` + padlock | 2.9 / 5.0 |
| viewport.shape.hidden | not drawn | not drawn | |
| viewport.picture | the bitmap at 100 %, 60 % when any shape is selected | same | |
| viewport.rapid (thin dashed) | `#8A959D` | `#6F7A82` | 2.8 / 4.3 |
| viewport.plunge (mark at a descent) | `#C62E27` | `#F2726A` | 5.1 / 6.6 |
| viewport.tab (heavy mark) | `#1C2128` | `#F5F7FA` | 15.1 / 17.5; a hollow (uncrossed) tab is the same stroke at 1.5 px with no fill |
| viewport.start (circle + tick) | the operation's colour | same | |
| viewport.drill (bit-sized circle) | the operation's colour | same | |
| viewport.direction-arrow | the operation's colour | same | |
| viewport.extents.centre (solid) | `#5B6472` | `#A3ABB7` | 5.6 / 8.1 |
| viewport.extents.edge (dashed, outside) | `#8A929E` | `#7B8491` | 2.9 / 5.0 |
| viewport.park (crossed circle) | `#5B6472` | `#A3ABB7` | |
| viewport.marquee | accent at 1 px, accent.soft fill | same | |
| viewport.handle (fill / stroke) | `#FFFFFF` / accent | `#0F1214` / accent | |
| viewport.label.bg (cursor readouts, extents caption) | `#FFFFFFE0` | `#1B2024E6` | |

The light Viewport's background is a shade darker than it was, which
leaves the male board, a locked shape, the extents' edge box and rapids a
hair under 3:1 (2.8 to 2.9) there; each is dashed or carries a glyph, so
none depends on its colour alone.

### Operation colours

The CLI's `PALETTE` (`crates/dc-job/src/preview.rs`) is Matplotlib's
tab10 hues in their order, darkened for the light Viewport (design open
question 5, accepted). The redesign moves three of the eight:
green goes yellower and cyan bluer, so no toolpath sits near the accent's
hue (159°), and orange a shade deeper for the new background. The order
and the names stay. The CLI adopted the light column on 2026-11-18, so
`dc_job::preview::PALETTE` and the light Viewport are one table
(`tests/theme.rs` and the CLI's `palette_matches_the_app_light_set` hold
them together); dark mode has its own set, as before.

| # | Name | CLI | Light | on `#F6F7F7` | Dark | on `#0F1214` |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Blue | `#1F77B4` | `#1F77B4` | 4.5 | `#5BA6EA` | 7.2 |
| 2 | Red | `#D62728` | `#D62728` | 4.7 | `#F0605A` | 5.8 |
| 3 | Green | `#4A930D` | `#4A930D` | 3.6 | `#9AD64B` | 10.8 |
| 4 | Purple | `#8E5FB8` | `#8E5FB8` | 4.4 | `#B48FE0` | 7.1 |
| 5 | Orange | `#D36500` | `#D36500` | 3.5 | `#FFA046` | 9.3 |
| 6 | Cyan | `#0B90B1` | `#0B90B1` | 3.5 | `#4FC3F0` | 9.3 |
| 7 | Brown | `#8C564B` | `#8C564B` | 5.5 | `#C8907F` | 6.9 |
| 8 | Pink | `#C94FA8` | `#C94FA8` | 3.8 | `#F08FD2` | 8.6 |

Operation 2 is red in both modes because the CLI's is, and the Warnings
ladder depends on red meaning *error*. The rule that keeps them apart:
status red appears only on chrome with a glyph; toolpath red appears only
as a line on the Viewport, never as text, never with a glyph. A V-Carve's
clearing-pass floor rings draw in the operation's colour at 55 % alpha
(spec 70: "lighter than the V-bit's lines").

An operation's colour is its place in the Operations list, counting
every operation, ticked or not (`dc_app::operations::colour_at`). Its
row's glyph, its editor's header, every tool run of its toolpath on both
boards, its target marks and its swatch in the Layers key read that one
rule, so they are one colour, and it changes only when the list is
reordered. The CLI's preview counts a colour per tool run instead, so
the two pictures differ in which operation has which colour on a project
with a clearing pass, a male board or an unticked operation.

### Depth ramp ("By depth")

Five stops interpolated in linear sRGB, pale at the stock top to deep at
the deepest cut. On dark the "dark" end is a saturated blue rather than a
black one, so the deepest cut still stands off the background; the hue
family is the same, so the legend reads the same in both modes.

| Stop | Light | Dark |
| --- | --- | --- |
| 0 (stock top) | `#AFC2EE` (1.7:1; pale by intent, the ramp's top is rarely cut) | `#E4ECFF` (15.9:1) |
| 0.25 | `#7C98E0` | `#BCCBF8` |
| 0.5 | `#4A68C9` (4.8:1) | `#93A8F2` (8.2:1) |
| 0.75 | `#2A44A6` | `#6E86EE` |
| 1 (deepest) | `#1A2E8C` (10.8:1) | `#4A66E8` (3.9:1) |

The legend bar (section 9) is the same ramp drawn vertically, 12 px wide.

## 6. Iconography

**Set: Lucide**, ISC licence, copyright 2026 Lucide Icons and
Contributors, with the Feather-derived icons under MIT (Cole Bemis)
(https://github.com/lucide-icons/lucide; verified against `LICENSE` on
2026-11-02). Design rules: 24 × 24 grid, 2-px centred stroke, round caps
and joins, 2-px corner radius, 1-px inner padding. Bundle the icon font
`lucide.ttf` and its codepoint map from the `lucide-static` package, loaded
as the egui font family `icons`; icons draw at **16 px** in rows, fields,
section headers and the status bar, **20 px** in the header and sheet
headers, and **24 px** on the rail. Tint is the text colour of the context (text.primary, secondary,
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
linked copy `component` (not a chain: the chain means the proportion
link); shape glyph, leading each shape row in text.secondary: closed
outline `square`, circle `circle`, **open path `spline`** (an open
curve; never a text badge, never the word "open"); proportion link
`link` (linked) and `unlink` (broken); drag grip `grip-vertical`; section chevron
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

Header: Undo `undo-2`, Redo `redo-2`, Continue `arrow-right`, Export
G-code `download`. Rail: Design `shapes`, Stock `box`, Tools `drill`,
Operations `route`, Export `file-output`, Warnings `triangle-alert`,
Appearance `sun-moon`, Help `circle-question-mark`. Design step: Add
drawing `file-input`, Add picture `image-plus`. Arrange group: Rotate
`rotate-cw-square` (its chevron `chevron-down`), Flip horizontal
`flip-horizontal-2`, Flip vertical `flip-vertical-2`. Viewport's
floating controls: Select `mouse-pointer-2`, Tab tool `bookmark-plus`,
Fit `maximize`, Zoom out `zoom-out`, Zoom in `zoom-in`, Layers `layers`.
Tools step: Tool Library `library-big`. Start screen: Open
`folder-open`. Status bar: units menu `chevron-down`. Toolpath popover:
Show all `eye`, Hide all `eye-off`. Viewport: zoom to selection `focus`.
Lists: New tool `plus`, Duplicate `copy`, Delete `trash-2`. Targets:
`target` (the Setup row and the tree's target mark), Edit `pencil`.
Carve note `info` in **text.secondary** (not status.info: a note is not
a hint). Required field `circle-x` inside the field. Export result:
success `circle-check`, Show in folder `folder-open`, Copy path `copy`,
Save setup sheet `file-text`. Examples: none; the thumbnail is the
picture.

## 7. Controls

Shared state language (applies to every control below):

| State | Look |
| --- | --- |
| default | `surface.sunken` fill, 1-px `border.default` outline, 8-px radius, text.primary |
| hover | outline → `border.strong`; buttons: `surface.hover` wash |
| focus | 2-px `focus.ring` outline outside the control, 2 px offset, following its shape (a pill's ring is a pill); a field being typed in turns its own outline `focus.ring` and shows the caret |
| active (pressed / open) | `surface.active` wash; dropdown chevron rotates |
| disabled | fill unchanged, outline → `border.default`, text → text.disabled, no hover; a tooltip gives the reason when the spec names one (tool-change Z on a post without `G53`) |
| error | outline → status.error stroke; a 12-px message under the field with `circle-x`; the unit suffix stays grey |
| warning | outline → status.warning stroke; message with `triangle-alert` |
| required (unset) | outline → status.error stroke; `circle-x` 16 px inside the field's right end, before the unit; placeholder *Required* in text.disabled; **no message line**; the file's message is the tooltip (functional 66b) |
| dimmed-but-editable | the operation with no targets (spec 59) and a disabled operation's row: text.secondary everywhere, controls fully live; never the disabled look |

Controls:

- **Text field with trailing unit.** 32 px tall, `surface.sunken`, 8-px
  radius, 10-px padding, value left
  in tabular figures, the unit right-aligned in text.secondary inside the
  outline. Typing `0.5 in` in a mm project converts on leave and the field
  shows `12.7 mm`; the unit never changes with the input. Validation on
  leave, by any means (Tab, Shift+Tab, Enter, a click anywhere in the
  window, a command's key, the window losing the focus: functional 52);
  only Escape reverts. One undo step per leave.
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
- **Dropdown.** 32 px, a field's fill and outline, chevron `chevron-down`
  right, opens a `surface.raised` menu (12-px radius, the one shadow)
  with 32-px rows on 8-px-radius surfaces, the current row ticked with
  `check`; keyboard:
  Space opens, type to jump, Enter commits. The tool dropdown shows the
  tool-type glyph, name, size and `(T2)`; its last row is *Add from
  library…*.
- **Checkbox.** 16-px box, 4-px radius, `surface.sunken` with a
  `border.strong` outline; checked fills `accent.fill` (`accent.hover`
  under the pointer) with a `check` in text.on-accent; label 8 px right;
  the whole 32-px row is the hit target. No tri-state in this phase.
- **Segmented control.** For 2–4 mutually exclusive options whose labels
  fit in 56 px each: Direction (Climb / Conventional), Z zero (Top /
  Bottom), colour mode (By operation / By depth), Strategy (Offset /
  Raster), Wall pass (Last / First / None), Mirror (two icon segments),
  Machining order. 32 px: a `surface.sunken` track with fully rounded
  ends and no outline; the selected segment is a `surface.raised` pill,
  3 px in from the track, with text.primary Medium and a 1-px
  `border.default` around it; the rest text.secondary, `surface.hover`
  under the pointer. Longer sets (Side:
  five values; Entry; Start point; Corner relief) use a dropdown. Radio
  buttons do not appear in the app.
- **Toggle.** Only for on/off *features* with sub-fields (Lead, Last pass,
  Cut-out, arc-fit tolerance "off"): a 36 × 20 pill, `surface.sunken`
  with a `border.strong` outline and a text.secondary knob when off,
  `accent.fill` with a text.on-accent knob when on; the sub-fields indent
  16 px below it and dim when off. Elsewhere a checkbox.
- **Disclosure section header.** 32 px, full width, no band (the pointer
  gives it a `surface.hover` wash on its own 8-px-radius surface),
  13-px SemiBold, `chevron-right` 16 px left; open state remembered per
  group. An open section leaves 20 px under its body before the next
  header. A located warning or error inside a *closed* section shows its
  glyph + count at the header's right end so nothing hides. In the
  operation editor a **closed** header also shows a summary of its
  values after the title: 12-px text.secondary, 8 px after the title,
  truncated at the end (`Strategy  Raster 0°, climb, wall last`); the
  summary is not shown when the section is open.
- **Editor header** (functional 66). 48 px, the title of the editor's
  card, with no band and no rule: kind glyph 16 px in the operation's
  colour, the name 13-px
  SemiBold as an inline-editable label (hover shows the text cursor and
  a 1-px `border.strong` outline; click, F2 or a double-click on the
  row makes it a field with the name selected), then `· Pocket` in
  text.secondary, then the operation's status glyph right-aligned. No
  enabled checkbox: that is the list row's.
- **Setup block** (functional 66a). The first section of every editor,
  titled *Setup*, with no chevron (it does not collapse) and the same
  32-px header. Its rows are ordinary rows; the label column is the same
  112 px. Below it the other sections start closed.
- **Targets row.** A 32-px row in Setup: label *Targets*, then, in the
  field column, `target` + `2 shapes · pocket, island` (one shape is its
  name alone, `outline`; where the names do not fit the row shows the
  count alone, `2 shapes`, never a name cut to a letter; the
  full list in the tooltip), a `triangle-alert` when a target carries a
  warning, and two 32 × 32 quiet icon buttons at the right end: `pencil`
  (Edit targets) and `ellipsis` (Use selection, Add selection). No list
  of names with ✕ buttons.
- **Proportion link.** A 28-px-wide column to the right of the W and H
  fields: a 1-px `border.strong` bracket from the middle of W's row to
  the middle of H's, with a 28 × 28 quiet toggle on it holding `link`
  (linked, text.primary) or `unlink` (broken, text.secondary). Tooltip
  *Keep proportions*. Nothing sits between W and H.
- **Split button** (the Arrange group's Rotate, in the Inspector's
  header). One 28-px quiet button, icon
  `rotate-cw-square`, and a 16-px chevron segment divided by a 1-px
  `border.default` rule; the chevron opens a 240-px popover with an
  *Angle* unit field and a secondary *Rotate* button.
- **List row (operations, tools).** 52 px, on its own surface with an
  8-px radius, 8 px in from the card's edges, and no rule between rows:
  `grip-vertical` in
  text.disabled (text.secondary on row hover), enable checkbox, kind glyph
  in the operation's colour (a 10-px colour swatch is drawn in the glyph's
  stroke), name 13 px, second line 12 px text.secondary (`Pocket · 6.35 mm
  end mill`), then `eye`, then a 24-px status cell. Hover: `surface.hover`.
  Selected: `accent.soft` fill and a 3-px `accent` bar at its left edge,
  short of the surface's rounded corners.
  Disabled operation: name in text.secondary, strike nothing.
- **Status cell.** One of: nothing; spinner; `triangle-alert` + count in
  status.warning; `circle-x` filled in status.error; `info` in status.info
  for an unused tool or the tool's red dot (spec 63: a 8-px filled
  status.error dot when a feed is missing). Hover lists the messages. A
  tool's row carries the warning mark and the error mark too, for the
  findings that name the tool, drawn by the painter an operation's row
  uses (`widgets::status`); the worst shows: an error, the red dot, the
  warnings, the hint.
- **Design Tree row.** 30 px, on its own 8-px-radius surface like a list
  row (hover `surface.hover`; selected `accent.soft` with the 3-px
  `accent` bar): `eye`, `lock` (hover-revealed unless
  locked), the 16-px shape glyph (`square`, `circle` or `spline`) in
  text.secondary, name, then the `component` badge for a linked copy,
  then the target count right-aligned in text.secondary (hover lists the
  operations). While an operation is selected, its targets' rows carry a
  3-px bar in the operation's colour at the surface's left edge and a
  `target` glyph before the count. In target editing the `eye` column
  becomes a
  checkbox. Section headers (Shapes, Tabs, Pictures) carry their own
  `eye` and `lock` acting on all rows; an empty section is its header
  alone, with no *None* row.
- **Buttons.** 32 px. Primary: an `accent.fill` pill with text.on-accent
  and 16-px padding, `accent.hover` under the pointer (34 px in the
  header), used once per view (Continue or Export G-code, Add to
  project, Import in a sheet); disabled, a `surface.sunken` pill with a `border.default`
  outline and text.disabled. Secondary: `surface.raised`, a 1-px
  `border.default` outline (`border.strong` and a `surface.hover` wash
  under the pointer), 8-px radius, 12-px padding, text.primary
  (Duplicate, Place tabs, Add from library…). Quiet: no outline, text or
  icon only, `surface.hover` on hover (the header's Undo and Redo, row
  actions, Show all / Hide all). Destructive: a secondary button whose label and icon are
  status.error text, at rest too, and whose outline turns status.error
  under the pointer (Delete). Icon-only
  buttons are 32 × 32 with a tooltip after 500 ms.
- **Chip.** A 24-px pill with 12-px text: `surface.raised` with a 1-px
  `border.default` outline and text.secondary. A status chip takes its
  status tint as fill and adds its glyph (a Warnings row's tier).
- **Card.** `surface.panel`, 14-px radius, no border, no shadow; its
  content keeps 16 px from its edges and its list rows 8. A card inside a
  card is `surface.raised` with a 1-px `border.default` outline at a
  10-px radius.
- **Inline validation message.** 12 px, under the field, 4 px gap, glyph
  then text, in the status's text colour; the file's message verbatim
  (`inlay_male_board_unset`'s message, not its code). The code appears in
  the Warnings drawer, not here. One message per field; errors win over
  warnings.
- **Help affordance.** The wireframes' "?" circle is replaced. Every
  field of the app has help: what it does, its unit, the values it takes
  (validation's own limits) and what leaving it empty means. It shows in
  two places. Hovering a field's *label* for 500 ms shows it as a
  tooltip, with the `.dcv` field name in JetBrains Mono on a second
  line; the label's underline (1 px dotted, text.disabled) appears on
  hover to say it has help; hovering the *field* shows the same help (a
  required field's tooltip is the file's message, with the help as a
  second tooltip under it). And while a field has the keyboard, its help
  is the **help line** at the foot of the panel that holds it, the step
  panel's, the Inspector's or a sheet's: a hairline, then `info` 16 px
  in text.secondary, the field's label in 13-px Medium and the help in
  12-px text.secondary, wrapped to the panel, 10 px above and below. The
  line is pinned: the panel's content scrolls above it, and it takes its
  room only while a field of that panel is focused, so a panel with no
  field in use loses nothing. `circle-help` is kept only in sheet
  headers as a link to the chapter.
- **Rail button.** 56 × 52, a 10-px radius: a 24-px icon over an 11-px
  label, the state mark at the icon's top right (section 2, "Rail").
  The current step's is filled with `accent.soft`, icon and label in
  `accent`. The window has no tab strip: the steps are the rail's.
- **Search field.** A text field with a leading `search` glyph and a
  trailing `x` clear when non-empty; filters as typed, 150 ms debounce.
- **Table rows (Tool Library, export programs, Warnings).** 34 px, striped
  with `surface.stripe` on 8-px-radius surfaces, header row text.secondary
  12 px with `border.default` under it, the Tool Library's columns
  resizable (a hairline divider at the left edge of every column but
  the name, `accent` and 2 px under the pointer, with the column-resize
  cursor: dragging it sets the width of the column at its right, down
  to the column's minimum, and the name takes the slack, never under
  120 px; the widths are kept in `gui.toml`), numbers
  right-aligned
  tabular, the tick column 32 px. "Already in project" rows show a `check`
  in text.secondary and the row in text.secondary. A Warnings row is
  read, not scanned, so its message is never cut: it wraps after the
  tier's chip and the code, the row growing by its lines, with its
  field at the row's right end when both fit one line and under the
  message when they do not.
- **Tool Library header.** The library's `mm` / `in` segmented control
  sits at the header's right, before *Import…* and *Export library…*.
  Every length and rate in the sheet follows it, the table's columns
  and the editing pane among them; the project behind the sheet keeps
  its own units.
- **Export foot.** The foot of the Export step's panel, pinned under
  its scrolling content: a hairline, then one row 8 px under it and
  16 px above the card's edge. At the right the step's one primary
  button, *Export G-code* with `download`, a 34-px accent pill, which
  writes the programs the step plans, asking for a folder first when
  none is chosen. At its left, in 12-px text, what it will do (*Ready
  to write 2 programs.*, text.secondary) or, while it is disabled (a
  sunken pill), why, with `circle-x` in status.error text (*1 error
  blocks export*; *Waiting for the toolpaths…*); the whole reason is
  the button's tooltip. A field's help line (above, "Help affordance")
  shows under it.
- **Export result.** A sheet, 560 px wide (functional 77a). Status is
  one 15-px SemiBold line with `circle-check` in status.success, the
  CLI's lines under it in 12-px text.secondary; then, scrolling when
  they are long: Files, a table of 30-px
  striped rows under a 24-px header row (number, file name in JetBrains
  Mono, tool, time right-aligned, the columns one width for every row),
  then the totals; then *Folder* and the folder as a 12-px mono line
  cut at its start with a `copy` icon button, over *Show in folder* and
  *Save setup sheet…*, both secondary; and *Run
  instructions*, a closed disclosure whose steps are numbered 13-px
  lines, each numbered once (the report's own number is taken off the
  line), and whose controller notes are callouts (`surface.stripe`
  fill, 10-px radius, `info` glyph in text.secondary). The footer holds
  *Done*, the sheet's primary pill, at the right. The sheet takes its
  height from its content, up to the window less 48 px above and below,
  at once when *Run instructions* opens. Closed, the step shows
  `circle-check` and *2 programs written.* under its readiness
  card, with a secondary *Show result* at the row's right. A failure is one
  line in status.error with `circle-x` under the readiness card, and
  no sheet.
- **Scrolling lists.** A list that goes on past the edge of its area
  fades its last 20 px into the surface under it: at the bottom while
  there is more below, at the top once it is scrolled. A row the edge
  cuts reads as *more*, never as a row sliced in two. A list that fits
  has no fade. The scroll bar is the thin overlay one (section 12).
- **Vendor group header** (Tool Library). A 32-px header inside the
  table, with no band, chevron left, the vendor in 13-px SemiBold and the
  count after it in text.secondary (`Amana · 4`); *No vendor* last.
- **Example card** (start screen). 144 px tall, `surface.raised`, 1-px
  `border.default`, 10-px radius, 12-px padding; a 72-px well of
  `viewport.stock.fill` across its top with the 96 × 56 thumbnail
  centred in it, the example's shapes drawn in `viewport.shape` at 1 px,
  fitted with 4-px inset; under it the name 13-px Medium and one 12-px
  text.secondary line. Hover: `surface.hover`;
  the whole card is the button.

## 8. Interaction patterns

- **Selection.** One selection, shared by the Design Tree, the Viewport
  and the Inspector's Properties. Click selects; Shift-click adds (tree: range);
  Ctrl/⌘-click toggles; a marquee on empty stock selects what it touches;
  Escape clears. The tree scrolls to show the first selected row when the
  Viewport selects. An operation row's selection is a second, independent
  selection (what the editor shows); clicking a toolpath sets it.
- **Hover.** Shapes under the cursor turn `shape.hover` at 1.25 px and the
  status bar shows the name; rows take `surface.hover`; hovering an
  operation's row previews its targets' highlight (section 9) until the
  pointer leaves; hovering a tree row's count highlights that operation's
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
  Inspector shows its width, height, shape. The Tab tool arms the
  cursor with a `+`; clicking an outline adds a tab; Escape disarms.
- **Inline rename.** Double-click a tree or operation name, press F2 with
  its row selected, or choose *Rename* from its context menu: the text
  turns into a field in place with the name selected; Enter commits,
  Escape reverts, blur commits. One undo step. The operation editor's
  header name is the same control.
- **Undo feedback.** Edit shows *Undo Set depth of "Outside cut"*. On
  Undo/Redo a 2-second toast slides in at the Viewport's bottom-centre
  (`surface.raised`, 13 px): *Undone: Set depth of "Outside cut"* with the
  affected operation's row flashing `accent.soft` once (300 ms). The
  Viewport and the editor update in the same frame.
- **Stale and regenerating.** The moment a change settles (250 ms after the
  last edit), the affected rows show the spinner and their old toolpaths
  drop to 35 % alpha; when a result lands, the new toolpath replaces it
  at full alpha with no fade. Nothing is hidden while waiting.
- **Progress.** The status bar's generation cell, at the bar's right
  end, reads *Generating 2 of 5…* with the spinner; a *Stop* quiet
  button is beside it while any operation is pending. After Stop the
  cell reads *Stopped* with a *Regenerate all* quiet button; that button
  exists nowhere else (not in View, not in the Warnings drawer). When
  generation is current the estimate is in its place. Import runs inside
  its sheet with a 4-px progress bar under the sheet header. The Export
  step says what its plan waits for, with the spinner, where the
  programs will be listed (*Waiting for the toolpaths to finish
  generating…*, *Building the programs…*), and *Export G-code* is
  disabled with the same words until it is there.
- **Floating controls fade.** While a drag is under way on the Viewport
  (a pan, a marquee, a move, a resize, a tab) its floating controls and
  the depth legend ease to 60 % and back, so the drawing shows through
  where the pointer is working. A press that has not yet moved 4 px is
  no drag.
- **Target editing** (functional 67). Entering it draws a 48-px bar
  across the top of the Viewport, `accent.soft` fill and a 1-px `accent`
  rule under it: `target` + *Editing targets of "Panel": click shapes to
  add or remove them* (13 px) on the left, *Cancel* (secondary) and
  *Done* (primary) on the right. The cursor over a shape is the pointing
  hand; a click toggles membership and the shape's highlight in the same
  frame; the editor's Targets row updates live. The rest of the window
  stays visible but inert (the header, the rail and the editor are
  dimmed to text.disabled and do not take clicks), so the mode is
  unmistakable; the step panel shows the Design Tree, whose rows toggle
  membership as the shapes do.
  Enter is Done, Escape is Cancel; Done commits one undo step.
- **Empty states.** The start screen (functional 79) is drawn on the
  Viewport's card in place of the stock, a 624-px column centred in it:
  the 18-px heading *Start with a drawing* and one 13-px text.secondary
  sentence; 24 px under them a row of three 40-px buttons, the primary
  *New from drawing…* with `file-input`, the secondary *New from
  picture…* with `image-plus`, and the quiet *Open…* with `folder-open`
  and its shortcut as the tooltip; 32 px under them **Examples** (13-px
  SemiBold) over three example cards in a row, 12 px apart (section 7);
  and, when there are any, **Recent** over up to four 32-px rows, each
  the file's name in 13 px and its folder's name after it in 12-px
  text.secondary, the whole path its tooltip, the row a quiet button. It has one
  primary button, and the header's *Continue* is the window's other
  accent: the two are never the same action. Empty lists show one 13-px
  text.secondary sentence centred in the list area, and at most two
  secondary buttons under it (the Tools list: *New tool*, *Add from
  library…*), never a graphic.
- **Destructive actions.** Delete and Duplicate never confirm; the
  toast names the undo. There are two confirmations. The first is unsaved
  changes on close, quit, open or new: a 420-px sheet titled *Save
  changes to "plaque"?* with one line, *Your changes are lost if you
  don't save them.*, and the buttons **Don't Save** (destructive, on the
  left), **Cancel** (secondary) and **Save** (primary) on the right;
  Enter is Save, Escape is Cancel. The second is the Tool Library's
  *Delete* (#581): the library is outside project history, so a deletion
  cannot be undone. The question replaces the table in place and names
  the ticked tools ("Delete 2 tools from your library: IDC 1/4 in
  Downcut, Amana 45771? Projects keep their copies."), with *Delete*
  (destructive) and *Cancel* in the footer; Escape is Cancel. Delete on
  a project tool in use is refused inline: the button disables and its
  tooltip names the operations.
- **Sheets (Import drawing, Add picture, Tool Library, unsaved
  changes).** A `surface.raised` panel centred over a scrim of black at
  50 %, 16-px radius, 24-px padding, the one soft shadow and no border,
  32-px header with title and `x`, the same sections, fields and
  validation as the panels, and a footer with the one primary action, a
  pill, on the right and secondary actions on the left. A file's path
  in a sheet is one mono line cut at its start, since its name is at
  its end; the whole path is its tooltip. A sheet that holds a table
  (the Tool Library, with its import review) resizes: an 8-px strip
  along each edge and an 18-px grip of three short strokes at its bottom
  right corner take the drag, with the resize cursor; the sheet stays
  centred, so the edge follows the pointer and the opposite edge moves
  the other way; the size is kept in `gui.toml`. A sheet that asks one
  question (unsaved changes) or holds one form (Import drawing, Add
  picture) keeps its fixed size. Never a second sheet above a
  sheet: the tool picker from an editor is a popover, the import review
  replaces the Tool Library's panes in place with a *Back*, and so does
  *Delete*'s question, with *Cancel*. Export is not a sheet: it is the
  Export step, and its result takes its plan's place there. A sheet's
  notes are only those that change a decision: no footnote restating how
  files are written, no list the status bar already shows. A sheet's
  primary action closes it: *Add to project* closes the Tool Library
  once the tools are in the project, the status bar saying what was
  added, so no scrim is left to take the next click; a refusal keeps
  the sheet, with its reason.
- **Setup sheet (HTML).** Not egui, so not themed: black text on white,
  a system sans-serif stack (`-apple-system, "Segoe UI", Inter,
  sans-serif`; Inter is not embedded, to keep the file small and
  self-contained), 11 pt body, 16 pt title, tables with 0.5-pt rules,
  numbers right-aligned with tabular figures where the font has them,
  one program per block with `page-break-inside: avoid`, page margins
  15 mm, `@page { size: auto }` so A4 and Letter both fit, a footer line
  with the project name, date and DreamCarve version. No colour carries
  meaning, so a black-and-white printer loses nothing; warnings carry
  the word *Warning* and the code.
- **Keyboard focus.** Tab walks top-left to bottom-right: the header,
  the rail (its five steps, then Warnings, Appearance and Help), the
  step panel, the Viewport, the Warnings drawer, the Inspector, the
  status bar; inside a sheet it cycles within the sheet. Ctrl/⌘ 1 to 5
  open the five steps from anywhere. Every focusable element shows the 2-px ring
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
  smear at fit. An arc is drawn as chords that leave it by under a
  quarter of a point at every zoom, so a circle reads as a circle; the
  Import drawing sheet's preview flattens at its own pixel, a tenth of a
  device pixel, and draws its circles as smoothly.
- **Selection and hover order.** Hover draws under selection; selection
  draws over toolpaths; toolpaths draw over the picture; the picture over
  the grid. **Targets of the selected operation** draw over toolpaths at
  2 px in the operation's colour; every other shape drops to 40 % alpha
  while an operation is selected, and returns when the operation
  selection clears. A shape that is both selected and a target draws
  selected (accent) with the 2-px target stroke beneath it as a halo. Selecting an operation (row or toolpath click) raises its
  toolpath to the top and draws a 4-px halo in its colour at 25 % alpha
  beneath it; other toolpaths stay at full alpha unless *Only the selected
  operation* is on.
- **Floating controls** (`redesign.md`, "The Viewport's floating
  controls"; functional 72). Three groups over the card, each a
  `surface.raised` pill with a 1-px `border.default` and the one shadow,
  12 px in from the card's edges; a control in a pill is 28 px.
  - *Top left*, a 36-px pill: *Select* (`mouse-pointer-2`) and the *Tab
    tool* (`bookmark-plus`), the one in use on `surface.active`; a 1-px
    rule; *Fit* (`maximize`), *Zoom out* (`zoom-out`), the zoom as a
    percentage in a 56-px quiet button, and *Zoom in* (`zoom-in`). The
    percentage is of 96 px to the inch, about the work's real size on
    most displays; a click on it sets 100 %, and each zoom button steps
    by a quarter about the pane's centre.
  - *Top right*, a 36-px pill, once the project has a toolpath: the *By
    operation / By depth* segmented control, 184 px wide, and *Layers*
    (`layers` and its label), pressed while its popover is open.
  - *Bottom left*, a 28-px pill: the cursor read-out, and the Z range
    after it in text.secondary while the program extents are shown.
    Neither showing, there is no pill.

  They ease to 60 % while a drag is under way (section 8). Fit keeps the
  stock clear of them: 60 px at the top, 64 px at the bottom (the
  read-out and, over it, the origin's `0,0`) and 40 px at
  each side, more where the Warnings drawer or an Inspector lies over
  the card. Where the pane is too narrow for both top pills, *Layers* is
  its icon alone (the Viewport at its least width, 520 px), and in a
  narrower one still the zoom controls go, leaving *Fit* and the wheel.
  Under the target-editing bar the top pills sit below it, and the
  drawing does not move when the mode starts.
- **Cursor coordinates.** The read-out at the pane's bottom left updates
  every frame in tabular 12 px: `X 64.2  Y 51.8` (one decimal in mm,
  three in inch), counting from the origin, in a fixed 124-px slot so
  the pill holds still. It is there only while the pointer is over the
  Viewport. No crosshair cursor and no readout chasing the cursor.
- **Male inlay board.** Drawn to the right of the stock with a gap of
  20 % of the stock's width (minimum 20 mm), as `--preview` places it:
  its own `viewport.male-board` outline, a smaller origin mark, the
  targets mirrored in `viewport.male-board`, the band and cut-out
  toolpaths in the Inlay's colour. Its shapes are not selectable; hover
  shows `Male board · Inlay 1` in the status bar; a click selects the
  Inlay. A `Male board` caption (12 px, label pill) sits at its top-left.
  Fit includes it.
- **Toolpath view popover.** Opened from *Layers* in the Viewport's
  top-right pill, 8 px under it with its right edge on the button's,
  240 px wide, `surface.raised`, 12-px radius, 1-px `border.default`,
  popover shadow; title *Toolpaths* 13 px SemiBold; then the **key** of
  the colour mode chosen in the pill: by operation, one 24-px row per
  operation with a 10-px swatch and its name (truncated), at most eight
  then *and n more*; by depth, one 12-px text.secondary line *Pale is
  shallow, dark is deep; the legend is on the Viewport*; then eight
  32-px checkbox rows (Cuts, Rapids, Plunges, Direction arrows, Start
  points, Tab marks, Only the selected operation, Program extents) and
  *Show all* / *Hide all* quiet buttons. Closes on a click elsewhere or
  Escape. Its state persists in `gui.toml`.
- **Depth legend** (functional 73a). Shown only in By depth mode, on a
  `surface.raised` card with a hairline and the one shadow, hung 8 px
  under the top-right pill with its right edge on the pill's, 12-px
  padding, title
  *Z* 12 px SemiBold with the unit once (`Z (mm)`). One vertical scale:
  a 12 × 40 px band of `viewport.rapid` with diagonal 1-px hatching for
  the non-cutting heights, ticks labelled `+10  clearance` and `+5  safe
  Z` (each distinct safe Z once), then the stock-top tick `0  stock
  top`, then a 12 × 160 px bar of the ramp with ticks at each
  operation's depth (`−6  Square recess`) and the deepest cut. Values
  are work-coordinate Z with a true minus and an explicit `+`. Labels
  that would overlap stack to the right. It never overlaps the stock at
  fit: Fit stands clear of it either beside it or under it, whichever
  draws the stock larger, so a wide stock in a narrow pane sits under
  the legend at nearly its full size and is not squeezed into a strip
  beside it.
- **Program extents.** The two boxes above; the caption names the edge box
  as the cutter's reach: *Program extents (bit edge, dashed)*, and steps
  under the box where a floating control or the legend would cover it.
  The Viewport's read-out shows the Z range in the same tabular style.
- **Not 1998.** No black background by default, no bevelled frame around
  the pane, no scroll bars (pan and zoom instead), no rulers, no
  coordinate readout chasing the cursor, no shadow under the stock, no
  crosshair cursor, no 1-px aliased lines (every stroke is anti-aliased
  through epaint's feathering), no neon green on black.

## 10. Motion

- Disclosure sections and the Warnings drawer: 150 ms ease-out on
  height; content clips.
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
- Focus is always visible: the 2-px ring at 5.4:1 (light) / 9.6:1 (dark)
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
  locked shapes carry the padlock; open paths carry the `spline` glyph
  and are drawn dashed.
  The eight operation colours were chosen so adjacent pairs differ in
  lightness as well as hue; a future per-operation colour override is out
  of scope but the token table makes it cheap.
- Reduced motion follows the OS setting where the platform exposes it;
  otherwise a `gui.toml` key. Zoom level of the UI follows the OS scale
  factor; the type scale is not user-adjustable in this phase.

## 12. egui realisation notes

Written against egui 0.37 (`corner_radius` was `rounding` before
0.31). Set everything once in a
`theme::apply(ctx, Appearance)` call at startup and on an OS appearance
change; no widget sets a colour literal (the one rule a reviewer greps
for: `Color32::from_rgb` outside `theme.rs` and the painter-glyph module
is a bug).

- **Appearance.** Dark is the default: `Prefs::theme` starts at Dark and
  `theme::follow` sets `o.theme_preference` from it each frame, so the OS
  appearance matters only once the user chooses *System*
  (`ThemePreference::System`; egui reports `ctx.theme()` from the window
  backend, eframe/winit). Build both `Visuals` and install with
  `ctx.set_style_of(Theme::Light, light)` and
  `ctx.set_style_of(Theme::Dark, dark)`.
- **Cards.** The rail, the step panel and the Inspector are `Panel`s
  whose own `Frame` is `surface.window` and carries the margin at the
  window's edge; the card is painted inside it (`widgets::card`), so the
  backdrop shows in the gaps. A resizable panel keeps 3 px of the gap at
  its inner edge as inner margin and 2 px as outer margin, and the
  Viewport keeps the other 3: egui draws the panel's resize line in that
  outer margin, and the shell gives it the accent at 2 px. In a window
  under 1280 px wide the Inspector is an `Area` over the Viewport's
  card (`card::floating`), with a surface under it that takes the
  clicks. egui clips to
  rectangles, so after a card's content is drawn its four corners are
  painted back in `surface.window` (`card::mask_corners`): this is what
  rounds the Viewport's wgpu callback, a list row's wash and a scroll
  area at a card's edge. Each mask is one path that starts at the corner
  and winds clockwise, so the tessellator's fan fills it though it is not
  convex, with its usual feathering along the arc.
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
- **Spacing.** `item_spacing (8, 4)`, `button_padding (10, 6)`,
  `window_margin 12`,
  `menu_margin 4`, `indent 16`, `interact_size (24, 32)` (egui's stock
  buttons, dropdowns, text edits and menu rows take their 32-px height
  from it), `icon_width 16`,
  `icon_width_inner 10`, `icon_spacing 8`, `combo_width 160`,
  `text_edit_width 160`, `tooltip_width 320`, `scroll: ScrollStyle::thin()`
  with `bar_width 6`, `floating true`, `floating_allocated_width 0`,
  `dormant_background_opacity 0`, `dormant_handle_opacity 0`,
  `active_background_opacity 0.4`, `interact_background_opacity 0.6` (the
  overlay scrollbar: it takes no room, so a field is one width whether
  or not its panel scrolls, and it shows only while the pointer is over
  its list, so at rest it does not lie over the fields' right edges;
  egui's is good enough, no custom one).
- **Visuals.** `dark_mode`, `window_fill` = surface.raised, `panel_fill` =
  surface.panel, `extreme_bg_color` = surface.sunken, `faint_bg_color` =
  surface.stripe, `window_stroke` = `(1, border.default)`,
  `window_corner_radius 16`, `menu_corner_radius 12` (a row's 8 plus the
  menu's 4-px margin, so the curves are concentric), `window_shadow` and
  `popup_shadow` = the one shadow, `Shadow { offset (0, 8), blur 24,
  spread 0, color #00000024 light / #00000059 dark }` (black at 14 % and
  35 %; `theme::popover` and `theme::sheet` build the frames that carry
  it), `weak_text_color` = text.disabled (placeholders),
  `selection.bg_fill` = accent.soft, `selection.stroke` = `(1, accent)`,
  `hyperlink_color` = accent, `error_fg_color` = status.error text,
  `warn_fg_color` = status.warning text, `striped true`,
  `button_frame true`, `collapsing_header_frame false`,
  `indent_has_left_vline false`, `slider_trailing_fill true`,
  `handle_shape Rect { aspect_ratio 0.5 }`, `text_cursor.stroke` =
  `(1.5, accent)`, `interact_cursor Some(PointingHand)` for buttons only.
  `widgets.noninteractive` = `{ bg_fill panel, weak_bg_fill panel,
  bg_stroke (1, border.default), fg_stroke (1, text.primary),
  corner_radius 8, expansion 0 }`; `inactive` = `{ bg_fill sunken,
  weak_bg_fill sunken, bg_stroke (1, border.default), fg_stroke
  text.primary }` (a stock dropdown is a field; egui's menus clear both
  for their rows); `hovered` = `{ bg_fill sunken, weak_bg_fill
  surface.hover, bg_stroke (1, border.strong), fg_stroke text.primary,
  expansion 0 }`; `active` = `{ weak_bg_fill surface.active, bg_stroke
  (1, focus.ring) }`; `open` = `{ bg_fill raised, weak_bg_fill
  surface.active, bg_stroke (1, focus.ring) }`. Set `expansion 0`
  everywhere so hover never grows a
  control. Disabled text: egui derives it by fading `fg_stroke`; override
  with `text.disabled` via `Visuals::gray_out` → set
  `widgets.noninteractive.fg_stroke` and use `ui.add_enabled` so egui
  picks it.
- **Focus ring.** `Style::interact` returns the `active` style for a
  focused widget, which would paint the pressed wash; keep `active`'s wash
  light and draw the ring separately. egui 0.37 does not say whether
  the focus came from the keyboard, so the shell keeps a flag set by Tab
  and the arrows and cleared by any pointer press (`shell::focus`). The
  ring is drawn in one place, after the frame (`focus::finish`): 2 px of
  focus.ring whose inner edge is 2 px off the control, at the control's
  own radius plus the offset, so a pill's ring is a pill. It goes round
  the focused widget's rectangle unless the control has said what its
  shape is (`focus::outline`, through `widgets::button::focus_ring`,
  `focus_ring_at` for a radius and `focus_ring_round` for a rectangle):
  a field says its frame, since the focus is the text inside it; a list
  or tree row its surface inside its slot; a bare checkbox its box; the
  Viewport its card. No widget draws a ring of its own. A surface that
  only takes clicks (a floating pill, the editor's header band) senses
  `CLICK` without `FOCUSABLE`, so Tab does not stop on it.
- **Custom widgets** (one module each under `widgets/`):
  `card` (the card, the card inside a card, and the corner masks),
  `button` (primary pill, secondary, quiet, destructive, and the focus
  ring's helpers), `checkbox`, `chip`, `icon` (the Lucide set),
  `unit_field` (TextEdit with a trailing unit label and the reset button
  drawn inside the same frame, commit on `lost_focus`),
  `segmented` (a pill track with `n` segments, equal until a label would
  be cut; also the pill tabs),
  `row` (a list row's surface: hover wash, selected fill and accent
  bar; an editor row's label, wrapped to two lines, and its message),
  `section` (the disclosure header: the chevron, the title, the closed
  summary and the trailing status glyph; `CollapsingState` animates the
  body), `scroll` (a scrolling list's fades), `toggle` (egui has no
  switch; 36 × 20 painter widget), `spinner`, `op_glyph` (painter-drawn
  operation-kind icons). The list and tree rows are drawn by their
  panels (`panels::operations`, `panels::tools`, `panels::tree`) on
  `row`'s surface. Tables are painted rows at fixed column widths, 34 px
  high, striped.
- **Dropdowns.** `ComboBox` with `width` set and `icon` replaced by the
  Lucide chevron via `ComboBox::icon`; rows are `selectable_value` with a
  leading `check` glyph drawn by a custom row closure. The tool dropdown
  is a custom popup (`Popup`/`Area` under the field) because its rows have
  glyphs and a trailing action.
- **Popovers.** egui's `Popup`/`Area` floats above everything in the same
  viewport, so popovers and the toolpath view popover are real; the limit is
  that they cannot leave the window (no OS-level popups). Menus come from
  `egui::menu` with the `menu_margin` above; their shortcut hints are a
  right-aligned `Small` label in text.secondary.
- **Text truncation.** `Label::new(..).truncate()` (`TextWrapMode::Truncate`)
  gives an ellipsis; use it on a row's name (a shape's, a tool's, an
  operation's), whose whole is its tooltip. What the user must read to
  act is never cut: a field's label wraps to two lines, a Warnings row's
  message and the target-editing bar's sentence wrap, and a popover
  grows to its longest row. Middle truncation (`plaque…nc`) does not
  exist; a file's path is cut at its start, since its name is at its
  end, and the tooltip shows the whole.
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

One item per rule of `redesign.md` and of the sections above, each short
enough to tick against a picture. A reviewer writes the pictures
(`cargo test -p dreamcarve-app --test review redesign_review --
--ignored`, into `review/final/`, dark and light at 1×) and ticks each
item with the picture that shows it, or the test that holds it where a
picture cannot. `review/final/README.md` is the list as last ticked.
Where `redesign.md` and this document disagree the item says which it
follows.

### The window

1. The window is six regions: header, rail, step panel, Viewport,
   Inspector, status bar.
2. The backdrop shows as the 8-px gaps between the cards and the 8-px
   margin round them.
3. A card is `surface.panel` with a 14-px radius, no border, no shadow,
   no title band and no rule under its title.
4. No more than three surface shades are in view (`redesign.md`,
   principle 2).
5. The header is 48 px on the backdrop: the six menus at the left, the
   project's name centred, Undo and Redo, then the primary button.
6. The name has no extension, and a dot before it while there are
   unsaved changes.
7. The generation chip shows in the header while toolpaths regenerate.
8. The rail is a 64-px card: five step buttons from the top; Warnings,
   Appearance and Help pinned to its foot.
9. The step panel is 312 px wide (440 on Export): the step's name in
   18-px SemiBold, one 12-px summary line, then its content.
10. The Inspector is there only while the step has something selected
    that has an editor; without it the Viewport takes the room.
11. The status bar is 28 px on the backdrop, each cell a quiet 24-px
    pill.
12. At 1280 × 768 the rail, the step panel and the Inspector are at no
    less than their minimum and the Viewport is at least 520 px wide.
13. In a narrower window the Inspector lies over the Viewport, with a
    hairline and the one shadow.
14. The primary button reads *Continue* with an arrow off the Export
    step. On it the header has none, and *Export G-code* with `download`
    is at the foot of the step's panel, disabled while an error blocks
    export, with the reason beside it.
15. One accent button is in a view (`redesign.md`, principle 3).

### The steps

16. The rail's steps read Design, Stock, Tools, Operations, Export.
17. A step's mark is nothing while it is empty, a check in
    status.success when complete, a dot in status.warning or
    status.error when it needs attention.
18. The current step's button is filled with `accent.soft`, its icon and
    label in `accent`.
19. Design: *Add drawing* and *Add picture*, two equal secondary
    buttons, over the Design Tree (Shapes, Tabs, Pictures).
20. Stock: Units first, then the stock's fields with *Fit to shapes*,
    then Origin; no Inspector.
21. Tools: *New tool* and *Add from library…*, the list, *Duplicate* and
    *Delete* under it, a quiet *Tool Library…* at the card's foot.
22. Operations: *Add*, *Duplicate*, *Delete*, then the list in cut
    order.
23. Export: the readiness card; the three settings sections closed, each
    with its values after its title; Programs; Program Details, closed,
    saying how many; Destination; File name.
24. While an error blocks export, a card in the error tint lists each
    finding with its field, and the readiness card counts the same
    errors.
25. After an export the result is in the plan's place: Status, Files,
    the folder, and *Run instructions* closed, each step numbered once.
26. With nothing to cut, the Viewport's card shows the start screen: a
    heading and one sentence, three buttons with one primary, three
    example cards with thumbnails, the recent files; no control floats.
27. An empty list shows one sentence, and at most two buttons under it.

### Colour

28. No colour literal is outside `theme.rs` and the glyph painters.
29. Every scene is captured dark and light, and nothing is dark on dark
    or light on light in either.
30. Text is at least 4.5:1 on its surface, disabled text and
    placeholders apart.
31. The accent is on the current step, the selected row, focus, the
    primary button, a link, and a box or switch that is on; nowhere for
    decoration.
32. No toolpath is near the accent's hue: operations 3 and 6 are a
    yellower green and a bluer cyan.
33. Status red, amber and blue never show without a glyph or a word.
34. Operation 2's red is only a line on the Viewport, never on chrome
    (section 5, "Operation colours").
35. The Viewport's card is darker than the cards in dark mode and a
    shade off white in light; the stock is a shade off it.

### Shape, size and type

36. Radii: card 14, sheet 16, menu and popover 12, a card inside a card
    10, control and row 8, checkbox 4; a pill's is half its height.
37. Fields, dropdowns, buttons and segmented controls are 32 px high;
    the header's primary button is 34.
38. A Design Tree row is 30 px, a Tools or Operations row 52 with two
    lines, a table row 34.
39. A section header is 32 px with no band: a chevron, then a 13-px
    SemiBold title, and while closed a 12-px summary of its values.
40. A closed section holding a located error or warning shows its glyph
    and count at its header's right end.
41. Text is Inter at 12, 13, 15, 18 or 22 px, or JetBrains Mono at 12;
    the rail's 11-px label is the one text under 12.
42. Labels take a 112-px column in every editor, 8 px from their
    fields, in text.secondary; none is cut, and a long one wraps to two
    lines in its row's height.
43. Sections in a card are 20 px apart, and a card's content is 16 px
    from its edges.
44. A card has no shadow; a menu, popover, sheet and floating pill has
    the one soft shadow.
45. Figures are tabular, so columns of numbers align; a unit sits inside
    its field at the right in text.secondary.
46. A length the app shows and does not take from a field has two
    decimals in mm and three in inches; a limit is rounded down.

### Components

47. A primary button is an `accent.fill` pill with text.on-accent.
48. A secondary button is `surface.raised` with a `border.default`
    outline and an 8-px radius; a quiet one is text and icon alone.
49. A destructive button is a secondary one whose label and icon are
    status.error.
50. A segmented control is a `surface.sunken` pill track; the chosen
    segment is a `surface.raised` pill in text.primary; no segment's
    label is cut.
51. A chip is a 24-px pill with 12-px text; a status chip has its glyph
    and its tint.
52. A field is `surface.sunken` with a `border.default` outline.
53. A field that refused what was typed has the error outline and the
    message under it with `circle-x`.
54. An unset required field has the error outline, `circle-x` inside it
    and *Required*, with no message line.
55. A `null` that takes the tool's value shows `tool's: 4 mm` as a grey
    placeholder; a field with its own value shows ⟲.
56. A checkbox is a 16-px box; checked, it is `accent.fill` with a
    check.
57. A switch is a 36 × 20 pill; the fields it turns on are indented
    16 px under it, and a row is indented only under a parent drawn
    with it.
58. A list row has no rule; the selected one is `accent.soft` with a
    3-px `accent` bar at its left.
59. A Design Tree row is the eye, the shape's glyph (`square`, `circle`
    or `spline`), the name and the target count; no row says "open".
60. A rail button is 56 × 52 with a 10-px radius (section 7;
    `redesign.md` says 48 × 52).
61. A sheet is `surface.raised` with a 16-px radius and 24-px padding
    over a scrim of black at 50 %, its primary button a pill at the
    footer's right.
62. No sheet opens over a sheet: the tool picker is a popover, and the
    library's import review and Delete question replace its panes.
63. A sheet's primary action closes it: after *Add to project* the next
    click lands where it is aimed.
64. A tooltip is `surface.raised` with an 8-px radius and 12-px text
    (`redesign.md`); a label's shows its help and its `.dcv` field in
    the mono face.
65. A dropdown has a field's fill and outline and a chevron; its menu
    is `surface.raised` with 32-px rows, the current row ticked with
    `check`.
66. The tool picker lists each tool's glyph, name, number and size, no
    name cut, with *Add from library…* last.
67. The operation editor's header is the kind's glyph in the
    operation's colour, the name, `· Kind`, and the worst status glyph
    at the right; it has no enabled checkbox.
68. Setup is first, has no chevron and never closes; every other group
    starts closed and says what it holds, a Cut-out its tool and tabs,
    or *None*.
69. The Targets row is `target`, then the count and names (one shape:
    its name), a pencil and `⋯`.
70. Properties shows X, Y, W, H with the chain link at the right of W
    and H; Rotate and the flips are in the Inspector's header.
71. The Tool Library: a search field, a type filter, the tools grouped
    by vendor in 34-px rows, the units control in the header, the
    editing pane at the right.
72. A list that goes on past its area fades there, and no row is
    sliced; no scroll bar lies over a field at rest.
73. An example card is 144 px with its thumbnail in a 72-px well.
74. The Warnings drawer is a card over the Viewport's bottom with a
    grab handle; a row is the tier's chip, the code in the mono face,
    the message whole, and the field; it has no Regenerate button.
75. A count reads right at one: *1 tool*, *1 operation*, *1 hint*.
76. No OK / Cancel / Apply row is outside the unsaved-changes sheet,
    the library's Delete question and the target-editing bar.
77. *Delete*, never *Remove*, names a deletion; the save prompt reads
    *Don't Save*, *Cancel*, *Save*.

### The Viewport

78. Top left, one pill: Select and the Tab tool, a rule, Fit, zoom out,
    the percentage, zoom in.
79. Top right, once there is a toolpath: *By operation / By depth* and
    *Layers*.
80. Bottom left: the cursor's read-out, with the Z range while program
    extents are on.
81. At Fit no floating control covers the stock, and the read-out does
    not cover the origin's `0,0`.
82. In By depth the legend hangs under the top-right pill, and Fit
    stands clear of it.
83. The Layers popover: its title, the key, eight checkbox rows, *Show
    all* and *Hide all*.
84. The stock is `viewport.stock.fill` with a dashed outline; the grid
    is on the stock only; the origin has its crosshair and `0,0`.
85. Lines are anti-aliased and keep their width at every zoom ("a fit
    view and a 4× zoom show the same stroke widths"), and a circle
    reads as a circle, on the Viewport, in the import preview and in an
    example's thumbnail.
86. A selected shape has the accent box, four corner handles and the
    rotation handle; a locked one grey handles and a padlock.
87. The selected operation's targets are drawn in its colour and the
    other shapes dimmed.
88. The male board is at the stock's right with its caption.
89. Program extents are two boxes with a caption that no control
    covers.
90. Target editing: a 48-px bar in `accent.soft` with its sentence
    whole, *Cancel* and *Done*; the rest of the window dimmed; the step
    panel showing the Design Tree with checkboxes.
91. Stale toolpaths are at 35 % alpha while their rows regenerate.
92. Nothing on the Viewport animates.

### Focus, menus and the status bar

93. After Tab the focus ring is 2 px of focus.ring, 2 px outside the
    control it names and following its shape: a field, a dropdown, a
    pill, a rail button, a row, a checkbox, a switch, a section header.
94. The ring shows for keyboard focus only, and one ring is drawn.
95. Tab walks the header, the rail (its steps, then Warnings,
    Appearance, Help), the step panel, the Viewport, the drawer, the
    Inspector and the status bar; nothing without a name is a stop.
96. No control's hit area is under 24 × 24 px.
97. The menus are quiet text buttons; a menu is `surface.raised` with a
    12-px radius and 32-px rows, each shortcut at its row's right; a row
    that is on, or the current choice, has a check.
98. The status bar reads, from the left, units, the counts or *No
    problems*, the message; from the right, the job and the pointer. It
    has no stock size, no dashes and no *Idle*.

### Motion and words

99. Hover washes, a disclosure and the drawer ease in 120 ms
    (`redesign.md`; 100 to 150 ms in section 10), and all of it is off
    when the OS asks for reduced motion.
100. Every string on screen is from the string table.

## 14. Anti-patterns

| Dated | Replace with |
| --- | --- |
| Bevelled, gradient or glossy buttons | Flat fill or outline, one wash on hover, an 8-px radius or a pill |
| OK / Cancel / Apply rows under a property page | Edit in place; Undo from Edit; the one sheet with buttons is unsaved changes |
| A modal over a modal | A popover inside the sheet, or the sheet's content replaced with a *Back* |
| Tabbed property sheets (General / Advanced / Tabs…) | Disclosure sections in one scrolling editor |
| A "?" button beside every field | Help on label hover, with the field name in mono |
| 16-px pixel icons on a grey toolbar strip | No toolbar: a rail of five steps, each command in the step or the Inspector it belongs to, one primary button |
| Status bar text at 11 px in sunken cells | 12 px in quiet pills on the backdrop |
| Raised 3-D frames around panels and the canvas | Borderless cards on the backdrop, 8 px apart, no rule under a title |
| Grid over the whole canvas, black background, crosshair cursor | Grid on the stock only, paper-coloured stage, arrow cursor, coordinates in the read-out at the pane's bottom left |
| Lines that scale with zoom until the picture is a blob | Screen-constant stroke widths |
| Red text as the only sign of an error | Outline + filled glyph + message |
| Radio-button groups for Climb / Conventional | A segmented control |
| A spinner dialog during generation | Spinners in rows, faded stale toolpaths, *Generating 2 of 5…* in the status bar |
| Confirm dialogs on Delete | Delete, toast, Undo |
| Dark mode by inverting the light palette | Two token columns designed together; the operation palette checked on both backgrounds |
| Nested tree indentation with dotted connector lines | Flat rows under three section headers; no connectors |
| A splash screen | The start screen in the Viewport's card |
| A file name repeated in a toolbar or a panel title | The project's name in the OS window title and the header's centre, without its extension |
| Rotate and mirror as fields in a property sheet | Arrange actions in the Inspector's header that act on the selection |
| A padlock between two fields for proportions | A chain link to the right of the pair, linked or broken |
| A text badge where a glyph would do (`open`) | A shape glyph with a tooltip |
| A list of target names with ✕ buttons | Targets highlighted on the drawing, edited in a mode on the drawing |
| Every setting of an operation in one long open form | Setup first; the rest in closed sections that summarise their values |
| A paragraph of validation text under a required field | *Required* in the field, the message in its tooltip and the drawer |
| A wall of CLI prose in a panel (*check says*, the export report) | Labelled facts, a status line, a file list, details on demand |
| A floating options card covering the drawing | A popover from the Viewport's *Layers* button that closes when done |
| A dialog that repeats what the status bar shows (warnings, totals) | Show it once, where it lives |
| Generic buttons "Discard" or "Yes/No" on a save prompt | *Save*, *Don't Save*, *Cancel*, named for what they do |
