# The redesign's review (slice D5)

Ticked 2026-11-19 against the build of this commit. The checklist is
`experience.md`, section 13: one item per rule of `redesign.md` and of
`experience.md`'s other sections. Each item names the picture in this
folder that shows it holds (each scene is here twice, `-dark-1x.png` and
`-light-1x.png`), or the test that holds it where a picture cannot, or
says *not met* and what is wrong.

The pictures are written by the app, not drawn:

```sh
cargo test -p dreamcarve-app --test review redesign_review -- --ignored
```

85 scenes, 170 pictures: `a` the start screen and each step of an empty
project; `b` each step on the plaque, a refused field, a tooltip; `c`
the Inspector on a shape, several, a locked one, an operation and an
Inlay with their groups closed, a tab, several, a picture; `d` the tool
editor on each kind of tool; `e` every menu and popover; `f` and `g` the
Viewport's floating controls, the depth legend, target editing and the
Warnings drawer; `h` every sheet; `i` the Export step ready, blocked and
done; `j` the window at 1280 × 768 and 1100 px wide; `k` the focus ring
on each kind of control; `l` the operation editor on each of the eight
kinds, every group open.

**100 items: 94 met, 6 not met** (4, 15, 34, 64, 65, 99). Four of the six
are places where `redesign.md` and `experience.md`, or two rules of one
of them, ask for different things; they are for the maintainer to
settle, and the build follows the one named.

## The window

- [x] 1. Six regions. `c04-inspector-operation-groups-closed`
- [x] 2. The backdrop is the 8-px gaps and margin. `b01-design`
- [x] 3. A card has no border, shadow, title band or rule. `b01-design`,
  `b04-operations`
- [ ] 4. No more than three surface shades in view. **Not met as
  written.** `c04` has six: the backdrop, the cards, the Viewport, the
  stock, a field (sunken) and a floating pill (raised). `redesign.md`
  rules out more than three in its principle 2 and names seven surface
  tokens in its own tables; the build follows the tables.
- [x] 5. The header: menus, the name centred, Undo and Redo, the
  primary button. `b01-design`
- [x] 6. The name has no extension, and a dot while unsaved.
  `g01-warnings-drawer` (the dot), `b01-design`
- [x] 7. The generation chip while toolpaths regenerate.
  `h09-sheet-unsaved-changes` (behind the scrim: *Generating 1 of 5…*)
- [x] 8. The rail: five steps; Warnings, Appearance, Help at its foot.
  `b01-design`
- [x] 9. The step panel: 18-px name, 12-px summary line, content; 440 px
  on Export. `b01-design` to `b05-export`
- [x] 10. The Inspector only with a selection. `b01-design` against
  `c01-inspector-shape`
- [x] 11. The status bar's quiet pills. `b04-operations`
- [x] 12. 1280 × 768 holds all four cards, the Viewport 536 px.
  `j01-window-1280x768`; `kittest::floating_controls_never_cover_the_stock_at_fit`
- [x] 13. Narrower, the Inspector lies over the Viewport.
  `j02-window-1100-wide`
- [x] 14. *Continue*, and *Export G-code* on Export, disabled while
  blocked. `b04-operations`, `i01-export-ready`, `i03-export-blocked`
- [ ] 15. One accent button in a view. **Not met on the start screen.**
  `a01-start-screen` has *New from drawing…* and the header's *Continue*.
  `redesign.md`'s principle 3 rules out two; `experience.md`, section 8,
  "Empty states", allows these two ("never the same action"). The build
  follows `experience.md`. Every other scene has one (a sheet's primary
  button is over a scrim that dims the header's).

## The steps

- [x] 16. Design, Stock, Tools, Operations, Export. `b01-design`
- [x] 17. Nothing, a check, or a dot. `a01-start-screen` (nothing),
  `b01-design` (checks), `c08-inspector-inlay-groups-closed` (amber dot),
  `g01-warnings-drawer` (red dots)
- [x] 18. The current step in `accent.soft` and `accent`. `b01-design`
- [x] 19. Design's two buttons over the tree. `b01-design`
- [x] 20. Stock: Units, the fields, *Fit to shapes*, Origin; no
  Inspector. `b02-stock`
- [x] 21. Tools' buttons, list and *Tool Library…*. `b03-tools`,
  `d01-tool-end-mill`
- [x] 22. Operations' buttons and list. `b04-operations`
- [x] 23. Export's cards and sections, each closed one saying its
  values. `b05-export`, `j03-window-1100-wide-export`
- [x] 24. The blocking card, and the readiness card counting the same.
  `i03-export-blocked`, `a05-export-empty`
- [x] 25. The result: Status, Files, the folder, *Run instructions*.
  `i04-export-done`, `i06-export-done-run-instructions-whole`
- [x] 26. The start screen. `a01-start-screen`
- [x] 27. An empty list's one sentence. `a01-start-screen` (*No shapes
  yet.*), `a03-tools-empty`, `a04-operations-empty`,
  `h08-sheet-tool-library-empty`

## Colour

- [x] 28. No colour literal outside `theme.rs`.
  `theme::no_colour_literal_outside_theme`
- [x] 29. Dark and light, nothing lost in either. Every scene, both.
- [x] 30. Text at 4.5:1. Section 5's ratios;
  `theme::tokens_match_experience_md` holds the tokens to its tables.
- [x] 31. The accent only where it means something. `c04`,
  `g01-warnings-drawer` (the code is a link)
- [x] 32. No toolpath near the accent. `b04-operations`, `f02-layers-popover`
- [x] 33. Status colours with a glyph or a word. `g01-warnings-drawer`,
  `i03-export-blocked`, `b06-field-refused`. The rail's dot and a tool
  row's dot are the marks `redesign.md` and functional 63 name; their
  tooltips say it in words.
- [ ] 34. Operation 2's red only as a line on the Viewport. **Not met as
  written.** The Operations list draws a row's kind glyph in the
  operation's colour (section 7, "List row"), and the Layers popover a
  swatch, so operation 2's are red on chrome: `b04-operations`,
  `f02-layers-popover`. It is `#F0605A`, close to status.error's
  `#F2726A`. Section 5's rule and section 7's contradict each other; the
  build follows section 7.
- [x] 35. The Viewport is the stage. `b01-design`, dark and light

## Shape, size and type

- [x] 36. The radii. `c04`, `h04-sheet-tool-library`, `e01-menu-file`,
  `i01-export-ready` (the readiness card)
- [x] 37. 32-px controls, a 34-px primary button. `b02-stock`
- [x] 38. Rows of 30, 52 and 34 px. `b01-design`, `b03-tools`,
  `h04-sheet-tool-library`
- [x] 39. A section header and its closed summary. `c04`,
  `c08-inspector-inlay-groups-closed`
- [x] 40. A closed section's located glyph and count. No picture: a
  located error or warning opens its section by itself, so none is
  closed in these scenes. `editor::summary::located_finding_opens_its_group`
  holds the count the header shows.
- [x] 41. The type sizes. `b01-design`, `i01-export-ready` (22 px)
- [x] 42. Labels: one column, never cut, wrapped to two lines.
  `l01-editor-profile` (*Through-cut allowance*),
  `i02-export-settings-open` (*Tool-change machine Z*, *Canned drill
  cycles*); `widgets::row::tests::every_field_label_fits_its_column_whole`
- [x] 43. 20 px between sections, 16 px of padding. `l02-editor-pocket`
- [x] 44. One shadow, on what floats. `f02-layers-popover`, `e01-menu-file`
- [x] 45. Tabular figures, the unit in the field. `d01-tool-end-mill`,
  `h04-sheet-tool-library`; `theme::figures_are_tabular_and_hyphens_are_not`
- [x] 46. Shown lengths at two and three decimals. `l07-editor-inlay`
  (*cone height 5.49 mm*), `c05-inspector-tab` (*82.85, 113.17*);
  `units::tests::shown_lengths_have_two_places_and_limits_round_down`

## Components

- [x] 47. The primary pill. `b01-design`
- [x] 48. Secondary and quiet buttons. `b03-tools`
- [x] 49. A destructive button. `b04-operations`, `c05-inspector-tab`
- [x] 50. A segmented control, no label cut. `b02-stock`,
  `l07-editor-inlay` (*Plunge / Ramp / Helix* in a clearing pass)
- [x] 51. Chips. `i03-export-blocked`, `g01-warnings-drawer`
- [x] 52. A field. `b02-stock`
- [x] 53. A refused field: outline, glyph, message. `b06-field-refused`
- [x] 54. An unset required field. `a02-stock-empty`,
  `d07-tool-tapered-ball-nose-unset`
- [x] 55. `tool's: 4 mm`, and ⟲. `l01-editor-profile`, `l07-editor-inlay`
  (*Floor stepover*)
- [x] 56. A checkbox. `l01-editor-profile`
- [x] 57. A switch and what is indented under it. `l01-editor-profile`
  (*Lead*, and *Tab width* at the margin under *Corner relief*);
  `editor::summary::a_row_is_indented_only_under_a_parent_drawn_with_it`
- [x] 58. A list row, the selected one's bar. `c04`
- [x] 59. A Design Tree row; no "open". `b01-design` (*squiggle*)
- [x] 60. A rail button, 56 × 52. `k02-focus-rail-step`;
  `kittest::focus_ring_goes_round_the_control_it_names`
- [x] 61. A sheet. `h01-sheet-import-drawing-svg`,
  `h09-sheet-unsaved-changes`
- [x] 62. No sheet over a sheet. `e12-popover-tool-picker`,
  `h06-sheet-tool-library-delete`, `h07-sheet-tool-library-import-review`
- [x] 63. A sheet's primary action closes it.
  `library::add_to_project_closes_the_sheet_and_the_next_click_lands`
- [ ] 64. A tooltip. **Not met as `redesign.md` writes it.**
  `b07-label-tooltip`: raised, 8-px radius, the `.dcv` field in the mono
  face, but its sentence is 13 px (section 12, "Tooltips": Body, then
  Monospace), not `redesign.md`'s 12.
- [ ] 65. A dropdown's current row ticked. **Not met.** `e13-dropdown-side`:
  the current row is on `accent.soft` in `accent`, with no `check`
  (egui's `selectable_value`). The fill, outline, chevron, menu and
  32-px rows hold.
- [x] 66. The tool picker. `e12-popover-tool-picker`
- [x] 67. The editor's header. `c04`, `l07-editor-inlay` (the warning's
  glyph)
- [x] 68. Setup, then closed groups that say what they hold.
  `c08-inspector-inlay-groups-closed` (*Cut-out  1/8 end mill, holding
  tabs, …*), `c09-inspector-inlay-no-cut-out` (*None*);
  `editor::summary::closed_cut_out_group_says_its_tool_and_tabs`
- [x] 69. The Targets row. `c04` (*outline*), `c08` (*2 shapes*)
- [x] 70. Properties and the Arrange group. `c01-inspector-shape`,
  `e14-popover-rotate`
- [x] 71. The Tool Library. `h04-sheet-tool-library`,
  `h05-sheet-tool-library-editing`
- [x] 72. Lists fade where they go on; no bar over a field.
  `h04-sheet-tool-library`, `h05`, `g01-warnings-drawer`;
  `library::library_table_fades_where_it_goes_on`
- [x] 73. An example card. `a01-start-screen`
- [x] 74. The Warnings drawer, an error, a warning and a hint in it.
  `g01-warnings-drawer`
- [x] 75. Counts at one. `c04` (*1 hint*), `l09-editor-photo-v-carve`
  (*1 operation*); `strings::tests::no_message_reads_one_things`
- [x] 76. No OK / Cancel / Apply row. Every scene; the three exceptions
  are `h09`, `h06` and `f04-target-editing`.
- [x] 77. *Delete*, and *Don't Save / Cancel / Save*. `b04-operations`,
  `h09-sheet-unsaved-changes`; `strings::no_deletion_is_labelled_remove`

## The Viewport

- [x] 78. The top-left pill. `f01-viewport-by-operation-read-out`
- [x] 79. The top-right pill, once there is a toolpath. `f01` against
  `a02-stock-empty`
- [x] 80. The read-out, and the Z range. `f01`,
  `f03-depth-legend-and-extents`
- [x] 81. Nothing floats over the stock or the origin's label. `f01`,
  `f03`; `kittest::floating_controls_never_cover_the_stock_at_fit`
- [x] 82. The depth legend. `f03-depth-legend-and-extents`
- [x] 83. The Layers popover. `f02-layers-popover`
- [x] 84. The stock, its grid and the origin. `b01-design` (light)
- [x] 85. Smooth lines and circles. `b05-export`,
  `h02-sheet-import-drawing-dxf` (the preview's circles),
  `a01-start-screen` (the coaster);
  `renderer::segment_quad_width_is_screen_constant`,
  `panels::import::tests::preview_flattens_a_circle_to_a_tenth_of_a_device_pixel`
- [x] 86. A selected shape's handles; a locked one's. `c01-inspector-shape`,
  `c03-inspector-locked-shape`
- [x] 87. The selected operation's targets. `c04`
- [x] 88. The male board. `l07-editor-inlay`
- [x] 89. Program extents and their caption. `f03-depth-legend-and-extents`
- [x] 90. Target editing. `f04-target-editing`
- [x] 91. Stale toolpaths. `h09-sheet-unsaved-changes` (the stock was
  just widened)
- [x] 92. Nothing on the Viewport animates. No picture can show it: the
  one animation in `viewport/` is the floating controls' fade
  (`controls.rs`), which section 8 asks for.

## Focus, menus and the status bar

- [x] 93. The ring, 2 px outside each kind of control.
  `k01-focus-primary-pill` to `k11-focus-switch`;
  `kittest::focus_ring_goes_round_the_control_it_names`
- [x] 94. For keyboard focus only, and one ring.
  `kittest::focus_order_and_visible_ring`
- [x] 95. Tab's order, and no stop without a name. The same two tests.
- [x] 96. Hit areas of 24 × 24 px.
  `kittest::focus_ring_goes_round_the_control_it_names`, for every
  control Tab reaches on the plaque's five steps.
- [x] 97. The menus. `e01-menu-file` to `e06-menu-help`,
  `e04-menu-view` and `e08-menu-appearance` (the checks)
- [x] 98. The status bar. `b04-operations`,
  `f01-viewport-by-operation-read-out` (the pointer's cell)

## Motion and words

- [ ] 99. Motion, and none when the OS asks. **Not met.** Hover washes
  and disclosures ease in egui's one `animation_time`, 150 ms
  (`redesign.md` says 120; section 10 says 100 to 150), and the app does
  not read the OS's reduced-motion setting or a `gui.toml` key for it
  (section 11).
- [x] 100. Strings from the table. `strings::every_key_is_used`,
  `strings::placeholders_are_filled`

## Seen and left

What the pictures show that no item above covers, and was not changed:

- **A blank project's two counts.** On the Export step of a blank
  project the status bar says *No problems* and the step says *1 error*
  (`a05-export-empty`). The status bar's rule is slice D4's decision (a
  blank project counts nothing); the step now counts what refuses
  export, so its two cards agree. The bar and the step still differ.
- **A menu's check column.** In the View menu the rows that can be
  ticked are indented by the check's column and the others are not
  (`e04-menu-view`).
- **A library row's name.** The table gives the name 120 px, so a name
  that repeats its vendor is cut (*Acme Router Bits…* under *Acme Router
  Bits*, `h05`). The whole name is in the pane's title and the row's
  tooltip.
- **The dark stock's grid** is 1.1:1 on the stock, as section 5 sets it,
  and hardly shows (`a04-operations-empty`); the light one reads
  (`a02-stock-empty`).
- **The older sets** in `review/d2/`, `d3/`, `d4/` and `d6/` are each
  slice's record and were not written again; they show the window
  before this review's fixes.
