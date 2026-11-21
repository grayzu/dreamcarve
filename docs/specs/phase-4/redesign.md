# Phase 4: the redesign

Status: decided with the maintainer on 2026-11-18 (`docs/alpha-worklog.md`,
"UI decisions"). It replaces the look and the layout that `experience.md`
and capabilities 50, 50a and 50b of `functional.md` describe. It adds no
capability and removes none: every panel, field and command of 51 to 80
still exists and keeps its label. Where this document and `experience.md`
disagree, this one wins until the slice that implements a section has
brought `experience.md` and `functional.md` into line with it; the
wireframes in `mockups/` show the old layout and are kept only for their
field names.

## What the maintainer asked for

A full redesign, not a restyle. Dark by default, light kept. One accent,
green/teal. Navigation rethought as a left icon rail and a step-by-step
flow. The reference is a folder of interface examples (not in the
repository). What they share:

- A dark backdrop with **rounded cards** floating on it, a few pixels
  apart. Depth comes from three close shades, not from borders.
- **One saturated accent** that marks where you are and what to press
  next. Everything else is grey.
- **Pills**: tabs, segmented controls, chips and primary buttons have fully
  rounded ends.
- **Room**: larger controls and padding than a classic desktop tool, with
  a clear order of title, summary number, then detail.
- An **inspector beside a canvas** in the ones that are editors.

## Principles

These replace principles 1, 5 and 6 of `experience.md`. Its principles 2
(numbers are the interface), 3 (one severity ladder) and 4 (the Viewport
is the picture of what the machine will do) stand unchanged.

1. **The app says what to do next.** The work is five steps in a fixed
   order: Design, Stock, Tools, Operations, Export. The rail shows which
   are done, which need attention, and which is current; one accent button
   in the header moves to the next step that needs something. Nothing is
   locked: any step can be opened at any time. *Rules out:* a wizard that
   blocks, a blank window with a menu bar, a toolbar of twelve equal
   buttons.
2. **Cards on a dark stage.** Every region is a rounded card on the window
   backdrop. A card has no border of its own. *Rules out:* dividers
   between regions, bevels, panel title bands with a rule under them,
   more than three surface shades in view.
3. **One accent, used sparingly.** The accent marks the current step, the
   selected row, focus, and the one primary button on screen. *Rules out:*
   two accent buttons in one view, accent text for decoration, accent on a
   toolpath.
4. **Roomy, still a tool.** 32-px controls and 13-px text. A panel shows
   one job at a time, so nothing needs to be small. *Rules out:* 26-px
   controls, text under 12 px, a second column of fields squeezed into a
   panel.
5. **Dark first; light is the same design.** Every token has both values,
   chosen together and contrast-checked.

## The window

Six regions. The backdrop (`surface.window`) shows as the 8-px gaps
between them and the 8-px margin round them.

| Region | Size | What it holds |
| --- | --- | --- |
| Header | 48 px high, full width | Menus (File, Edit, Arrange, View, Project, Help) as quiet text buttons at the left. The project's name in the centre with a dot while there are unsaved changes; it also stays in the OS window title. Undo, Redo, the generation chip, and the primary button at the right. |
| Rail | 64 px wide, a card | The five step buttons, each a 24-px icon over an 11-px label with a state mark. Pinned to the bottom: Warnings (with its count), Appearance, Help. |
| Step panel | 312 px wide (min 280), a card | The current step's content: its title, one summary line, then its lists and fields. |
| Viewport | The rest (min 520), a card | The drawing, with its tools and view chips floating over it. |
| Inspector | 328 px wide (min 300), a card | The editor for what is selected in the step: a shape's Properties, a tool, an operation. Absent when the step has nothing selected, and the Viewport takes the room. |
| Status bar | 28 px high, full width, on the backdrop | The cells of 50b, unchanged in content, each a quiet pill. |

Minimum window 1280 × 768, as before: 64 + 280 + 520 + 300 and five 8-px
gaps fit in 1280 with room to spare. Below that the Inspector overlays the
Viewport instead of the window refusing. The step panel and the Inspector
resize by dragging their inner edge; the gap is the handle and shows the
accent on hover.

The primary button reads **Continue** and moves to the next step that is
not complete. On the Export step it reads **Export G-code** and is the
export command; it is disabled, with the reason in its tooltip, while an
error blocks export.

### The steps

A step's state is computed from the project and its findings, in `dc-app`,
so it is tested without a window.

| Step | Step panel | Inspector | Complete when | Needs attention when |
| --- | --- | --- | --- | --- |
| Design | *Add drawing* and *Add picture*; the Design Tree (Shapes, Tabs, Pictures) | Properties of the selection, with the Arrange group in its header | The project has a shape or a picture | A drawing's import notes hold a warning |
| Stock | Units; Stock (width, height, thickness, margin, *Fit to shapes*); Origin (XY zero, Z zero); the male board when an Inlay needs one | None | Thickness is set and every shape is on the stock | A stock or origin field has a problem |
| Tools | The project's tools, *Add tool*, *Tool Library…* | The tool editor | A tool exists and none an enabled operation uses lacks a feed, plunge rate or speed | A tool has an unset required value |
| Operations | The operations list with *Add*, enable, visibility, reorder | The operation editor | An enabled operation exists and none has an error | An operation has an error or a warning |
| Export | A readiness card (errors, warnings, estimate, Z range); post-processor and output settings; the programs to be written; after export, what was written | None | The last export succeeded and nothing changed since | Anything blocks export |

The state mark on a rail button: nothing while the step is empty, a check
in `status.success` when complete, a dot in `status.warning` or
`status.error` when it needs attention. The current step's button is
filled with `accent.soft` and its icon and label take `accent`.

Project Settings and the CAM panel no longer exist as panels: Project
Settings' fields are the Stock step and the settings half of the Export
step, and the CAM panel's two tabs are the Tools and Operations steps.
The Export sheet becomes the Export step; export still asks for a folder
with the native dialog. The Import sheet, the Tool Library sheet and the
unsaved-changes sheet stay sheets. The Warnings drawer stays a drawer
over the bottom of the Viewport card, opened from the rail or the status
bar.

Opening a project selects the first step that is not complete, so a
finished project opens on Export and an empty one on Design.

### The start screen

When the project has no shapes, pictures or operations, the Viewport card
shows the start screen in place of an empty stock: a heading (*Start with
a drawing*), one sentence, two buttons (**New from drawing…** primary,
**New from picture…** secondary), a quiet *Open…*, the three example
cards (coaster, plaque, sign) with their thumbnails, and the recent files.
A drawing, picture or `.dcv` dropped anywhere on the window does what the
matching button does. The step panel shows the Design step with its empty
sentences.

## Tokens

All values sRGB hex; contrast is WCAG 2, computed. Token names are
`experience.md`'s, so `theme.rs` and the test that reads the tables keep
working; `accent.hover` is new.

### Surfaces, borders, text, accent

| Token | Light | Dark | Notes |
| --- | --- | --- | --- |
| surface.window | `#ECEFF1` | `#0C0F11` | The backdrop |
| surface.panel | `#FFFFFF` | `#14181B` | Cards |
| surface.raised | `#FFFFFF` | `#1B2024` | Menus, popovers, sheets, a card inside a card |
| surface.sunken | `#F3F5F6` | `#0F1315` | Fields, lists |
| surface.stripe | `#F7F8F9` | `#181D20` | |
| surface.hover | `#141A1D10` | `#E8EDEF10` | 6 % of text.primary |
| surface.active | `#141A1D1A` | `#E8EDEF1A` | 10 % |
| border.default | `#DCE2E5` | `#252B30` | Field outlines at rest, rules inside a card |
| border.strong | `#B8C2C8` | `#3A434A` | Field outline on hover |
| text.primary | `#141A1D` (17.6:1 on panel) | `#E8EDEF` (15.1:1) | |
| text.secondary | `#52606A` (6.5:1) | `#9AA6AD` (7.2:1) | |
| text.disabled | `#97A2A9` | `#66727A` | |
| text.on-accent | `#FFFFFF` (5.4:1 on accent.fill) | `#04261B` (7.4:1) | Dark text on the bright fill in dark mode |
| accent | `#08795E` (5.4:1 on panel) | `#2AD79A` (9.6:1) | Icons, links, the current step |
| accent.fill | `#08795E` | `#22C78C` | Primary button |
| accent.hover | `#0A8A6C` | `#3FE0A8` | Primary button on hover |
| accent.soft | `#08795E1F` | `#2AD79A29` | Selected rows, the current step, text selection |
| focus.ring | `#08795E` | `#2AD79A` | 2 px, 2 px off the control |

The status tokens keep `experience.md`'s values, except `status.success`
in dark, which becomes `#5FD08A` so it does not read as the accent. Status
red, amber and blue never appear without a glyph or a word.

### Viewport

The Viewport card is the stage: darker than the cards in dark mode, a
shade off white in light.

| Token | Light | Dark |
| --- | --- | --- |
| viewport.bg | `#F6F7F7` | `#0F1214` |
| viewport.grid.minor | `#ECEFF0` | `#15191C` |
| viewport.grid.major | `#DDE2E5` | `#1D2226` |
| viewport.stock.fill | `#FFFFFF` | `#161B1E` |
| viewport.axis, viewport.stock | `#6B7780` | `#9AA6AD` |
| viewport.shape | `#1E2529` | `#E6EAEC` |
| viewport.shape.hover | accent.hover | accent.hover |
| viewport.shape.selected | accent | accent |
| viewport.rapid | `#8A959D` | `#6F7A82` |
| viewport.handle (fill / stroke) | `#FFFFFF` / accent | `#0F1214` / accent |
| viewport.label.bg | `#FFFFFFE0` | `#1B2024E6` |

Tokens not listed keep their `experience.md` values.

### Operation colours

Two of the eight move so no toolpath sits near the accent's hue (159°):
green goes yellower and cyan goes bluer. The order and the names stay, so
the CLI's preview still matches the screen by eye.

| # | Name | Light | on `#F6F7F7` | Dark | on `#0F1214` |
| --- | --- | --- | --- | --- | --- |
| 1 | Blue | `#1F77B4` | 4.5 | `#5BA6EA` | 7.2 |
| 2 | Red | `#D62728` | 4.7 | `#F0605A` | 5.8 |
| 3 | Green | `#4A930D` | 3.6 | `#9AD64B` | 10.8 |
| 4 | Purple | `#8E5FB8` | 4.4 | `#B48FE0` | 7.1 |
| 5 | Orange | `#D36500` | 3.5 | `#FFA046` | 9.3 |
| 6 | Cyan | `#0B90B1` | 3.5 | `#4FC3F0` | 9.3 |
| 7 | Brown | `#8C564B` | 5.5 | `#C8907F` | 6.9 |
| 8 | Pink | `#C94FA8` | 3.8 | `#F08FD2` | 8.6 |

The depth ramp is unchanged.

## Shape, size and type

| Thing | Value |
| --- | --- |
| Card radius | 14 |
| Sheet radius | 16 |
| Control radius (field, dropdown, secondary button, list row) | 8 |
| Pill (primary button, tab, segmented control, chip, status-bar cell, badge) | Half its height |
| Gap between cards, window margin | 8 |
| Card padding | 16 |
| Gap between a label and its field, between adjacent buttons | 8 |
| Gap between sections in a card | 20 |
| Control height (field, dropdown, button, segmented control) | 32 |
| Primary button in the header | 34 |
| Design Tree row | 30 |
| Tools and Operations row | 52, two lines, its own 8-radius surface on hover and when selected |
| Table row | 34 |
| Section header | 32, no band: a 13-px SemiBold label with a chevron, 20 px above it |
| Step title | 18-px SemiBold, with a 12-px secondary summary line under it |
| Summary number (estimate, stock size on the Export step) | 22-px SemiBold, tabular |

Type stays Inter and JetBrains Mono at `experience.md`'s sizes, with two
additions: the 22-px summary number and the 11-px rail label, the one text
under 12 px, allowed because an icon carries the meaning. Shadows: a card
has none; a menu, popover or sheet has one soft shadow (0, 8, blur 24,
black at 35 % in dark and 14 % in light).

## Components

- **Card.** `surface.panel`, radius 14, padding 16, no border. A card
  inside a card (the readiness card on Export, an example card on the
  start screen) is `surface.raised`, radius 10.
- **Buttons.** Primary: an `accent.fill` pill with `text.on-accent`; one
  per view. Secondary: `surface.raised` with a `border.default` outline,
  radius 8. Quiet: text and icon only, `surface.hover` behind it on hover.
  Destructive actions are secondary buttons whose label and icon take
  `status.error`.
- **Pill tabs and segmented controls.** A `surface.sunken` track, fully
  rounded; the chosen segment is a `surface.raised` pill with primary
  text, the others secondary text. Used for *By operation / By depth*, Z
  zero, units, and the tabs inside the Tool Library.
- **Chip.** A 24-px pill, `surface.raised`, 12-px text: counts, an
  operation's kind, *open* on an open path, a filter. A status chip adds
  its glyph and takes its status tint as fill.
- **Field.** `surface.sunken`, radius 8, 32 high, a `border.default`
  outline that turns `border.strong` on hover and `focus.ring` on focus.
  The unit sits inside at the right in secondary text. A problem turns the
  outline to its status stroke and puts its status glyph inside the field;
  the message takes no room: it is the glyph's tooltip, a row in the
  Warnings drawer (a click on the glyph opens it there), and, while the
  field has focus, the first thing in the panel's help line.
- **List row.** No rule between rows. Hover is `surface.hover`; selected
  is `accent.soft` with a 3-px `accent` bar at its left edge.
- **Rail button.** 48 × 52; icon 24 over an 11-px label; state mark at
  the icon's top right.
- **Sheet.** `surface.raised`, radius 16, 24-px padding, over a scrim of
  black at 50 %. Footer buttons right-aligned, the primary one a pill.
- **Tooltip.** `surface.raised`, radius 8, 12-px text.

## The Viewport's floating controls

Nothing sits in a toolbar strip any more. Over the Viewport card, each
group a `surface.raised` pill or card with the menu shadow, 12 px in from
the card's edge:

- **Top left:** Select, Tab tool, then Fit and zoom out, the zoom
  percentage, zoom in.
- **Top right:** the *By operation / By depth* control and a *Layers*
  button that opens the toolpath view popover of capability 73. The depth
  legend hangs under it while By depth is on.
- **Bottom left:** the cursor read-out and, while program extents are on,
  the Z range.

They fade to 60 % while a drag is under way on the Viewport, and never
cover the stock at Fit.

## Motion

120 ms ease-out for hover fills, the rail's current-step change, the
Inspector appearing, and the drawer; none on Viewport geometry. All of it
off when the OS asks for reduced motion.

## egui notes

`CornerRadius` and `Frame` give the cards and pills; `Frame::shadow` gives
the one shadow. egui has no backdrop blur and no gradient fills, so the
design uses neither. The rail, the step panel and the Inspector are
`SidePanel`s with their frames set to no fill and a card frame drawn
inside, so the backdrop shows in the gaps. Rounded clipping of the
Viewport's wgpu callback is done by painting the four corner masks in
`surface.window` after the callback. Every colour still comes from
`theme.rs`; `theme::no_colour_literal_outside_theme` stays green.

## Slices

| Slice | Scope | Proof |
| --- | --- | --- |
| D2 | Tokens, shape and components in `theme.rs` and `widgets/`, applied to the existing layout. `experience.md` sections 3, 5 and 7 brought into line. Dark becomes the default appearance; the preference still overrides. | Theme tests green against the new tables; a light and a dark screenshot of the workspace. |
| D3 | `dc_app::steps` (the step states and the next step) with its tests. The header, the rail, the step panel, the Inspector and the start screen in `dreamcarve-app`. Capabilities 50, 50a and 50b rewritten. | `dc_app::steps` tests; window tests re-pointed at the steps; screenshots of each step, light and dark. |
| D4 | Export as a step; the Viewport's floating controls; the drawer and the sheets restyled; the status bar's pills. | Window tests; screenshots of Export before and after an export, the drawer, each sheet. |
| D5 | The review: every screenshot checked against this document, fixes, the snapshot pair regenerated, `experience.md`'s checklist rewritten for the new design. | The checklist, each item with its screenshot. |
