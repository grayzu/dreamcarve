# Phase 5 worklog

The local record of phase 5 (GUI, CAD), started 2026-11-22. It stands in
for GitHub issues and pull requests, as `docs/alpha-worklog.md` did for
phases 3 and 4. One line per slice; details live in the commits and in
`docs/specs/phase-5/`.

## Process

Asked by the maintainer on 2026-11-22: the approach of the alpha push,
for phase 5.

- All work is on the branch `Phase-5`, cut from `main` at `5f75ce9b` and
  pushed to GitHub as a backup. No pull requests, no issues, no CI waits.
  It merges to `main` only when the whole phase is done.
- The branch is version `0.5.0-alpha.1`.
- Spec first: `functional.md`, `design.md` and ADR-0006 were written
  before any code, by the agent, for the maintainer to review with the
  working build. A spec changes in the commit that changes what it
  describes.
- Test first. The gate before a slice is recorded as done, on macOS:
  `cargo fmt --all -- --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo nextest run --workspace` with
  `cargo test --workspace --doc`, and `cargo run -p check-headers`.
- Windows and Linux are tested by CI when the branch reaches `main`. The
  code stays portable meanwhile: no macOS-only API without a `cfg`, no
  path or line-ending assumptions. Before that merge, the checks CI runs
  and the local gate does not are run here too (`cargo doc`, `cargo
  deny`, `mdbook build`, the licence and dist checks, clippy for the
  Windows and Linux targets).
- The maintainer's own steps wait for them: the review of the build, the
  bench check, the decisions listed below, and the tags.

Reference checkouts, beside this worktree: `../purecutcnc` at `d04ae2f`
and `../graphite` at `615b4050` (sparse). Their surveys are
`docs/research/purecutcnc-drawing-digest.md` and
`docs/research/graphite-tools-digest.md`.

## Where it stands

Baseline before the first slice: 3,430 tests pass on macOS at
`0.5.0-alpha.1`. After slice 27: 3,727; after the maintainer's rounds of
requests, 3,799. Every slice is done. The maintainer tested the branch and
on 2026-11-25 gave the word to release it as `v0.5.0-alpha.1` (slice 28);
`v0.5.0` stays theirs.

### Exit criteria (functional spec)

| Criterion | Where it stands |
| --- | --- |
| Acceptance tests 107–130 green on macOS | Green: each is named in the design spec's test plan, and the gate passes. Windows and Linux run in CI when the branch reaches `main`. |
| S21's equality test (128) on all six post-processors | `gui_equality::the_hook_plate_drawn_equals_the_cli` |
| The hook plate drawn in the app with no import, its programs equal to the CLI's | `first_design::the_hook_plate_is_designed_in_the_window` draws it through the window; the same steps with no window are exported against `dreamcarve export` |
| Quick start "Your first design" and every chapter, with screenshots from the build | Written; 28 pictures taken by `book.rs` |
| `CONTEXT.md` updated; every user-visible string keyed | Done; `strings::every_key_is_used` and the scans pass |
| The maintainer's review, and the open questions settled | Tested by the maintainer, who gave the word to release on 2026-11-25. The open questions below stand as taken unless they say otherwise |
| `v0.5.0-alpha.1`, then `v0.5.0` | `v0.5.0-alpha.1` cut on 2026-11-25 at the maintainer's word (slice 28). `v0.5.0` is **the maintainer's** |

### Checks CI runs that the local gate does not

Run on macOS on 2026-11-23, at the head of the branch, and again on
2026-11-25 before the release (all clean on the first try that day):

| Check | Result |
| --- | --- |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace`, and with `--document-private-items` | Clean, after three intra-doc links were fixed |
| `cargo deny check licenses advisories` | OK, after `dc-text` left `ttf-parser` (below) |
| `mdbook build docs/book` | Builds |
| `cargo test --locked -p third-party-licenses --test ci -- --ignored` | Both pass; on 2026-11-25 with `FONT-LICENSES` checked in both archives |
| `dist plan`, `dist generate --check` | Both exit 0 |
| `cargo clippy --all-targets --all-features -- -D warnings` for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu` | Clean for both |

Not run here: `gcode-conformance` (LinuxCNC's `rs274`, CI only), and the
tests themselves on Windows and Linux.

## Decisions waiting for the maintainer

The functional spec's "Open questions" (ten) and the design spec's
(four). Each has a choice taken so the work can proceed; none blocks.
Decisions made while building are added here.

- Slice 1: a size that cannot be is `out_of_range` at the shape's own
  field, not a new `invalid_shape` code, so the app marks the field as it
  marks any other; a bitmap's opacity the same. The design spec says so
  now.
- Slice 2: a gear's sizes are checked, not clamped as PureCutCNC clamps
  them before it validates; an ellipse is drawn as chords of the true
  curve (its four axis points among the vertices), not as PureCutCNC's
  four Béziers flattened; a rounded corner of exactly half the shorter
  side is allowed, the side between the two arcs left out, where
  upstream stops a micrometre short.
- Slice 4, snapping, where it leaves PureCutCNC: a click with no snap
  is taken at the pointer (upstream discards it); the grid has no reach,
  so with grid snap on a point with nothing better near lands on the
  grid (Graphite's rule); the snap grid's spacing follows the zoom in
  steps of 1, 2 and 5, at least 8 px apart, or is pinned by the user,
  while the grid phase 4 draws (1, 10, 100 mm) stays as it is; `Shift`
  holds a direction to 15° steps and `Ctrl`/`⌘` suspends snapping
  (upstream cycles an axis lock on an `Alt` tap); Intersection is on by
  default (upstream: opt-in); a shape's real outline snaps, not its
  frame.
- Slice 5: on Rectangle and Ellipse `Shift` squares the box, so it
  does not also hold the corner to a 15° direction; a polygon measured
  to its flats takes the click as the middle of a side, not a corner; a
  slot whose width is typed with its length and angle is made at once,
  as PureCutCNC makes it.
- Slice 6: Line, Arc and Spline are one path's three kinds of segment,
  so changing among them keeps the path being drawn (upstream's
  composite tool; its separate polygon and spline tools are folded in).
  A path is named *Line*, *Arc* or *Spline* when its segments are all
  of that kind, else *Path*. A double-click finishes any of the three
  (upstream: a polygon only). A typed radius bows the arc to the
  pointer's side and is the short way round; an arc past half a turn
  is drawn through a point. `Enter` takes the typed point while a
  length or an angle is typed, and finishes the path when none is. A
  closed path is saved in the direction it was drawn, not turned
  counter-clockwise as the sized shapes are.
- Slice 7: a gear's size is held as it was typed and the reason said at
  its field, the gear not made while one stands (upstream's way); a
  typed size is not thrown back. A radius click on the centre is not
  taken (upstream raises it to the least size). The reasons that name a
  limit give it in project units; the rest are `dc-geometry`'s words.
  The module is shown in project units in an inch project too; a
  diametral pitch is not offered.
- Slice 8: the design named a `Reshape` command for a stretch that
  changes sizes; it is done inside `TransformInstances` instead, so a
  stretch of several shapes is still one undo step and every caller
  (Properties, the handles) gets it. `SetOutline` names the instance,
  not the definition, so its undo step can name the copy that was
  edited. Stretching a polygon or a gear is refused outright, as the
  functional spec says; the design's "lines stretch through the
  transform" now holds only for shapes that keep no sizes. A stretched
  circle keeps the name *Circle 1*. A locked linked copy makes the
  sizes of all its copies read-only. Convert to Path is allowed on a
  locked shape, since nothing moves.
- Slice 9: the tool bar floats under the pills, not across the top as
  the target-editing bar does, and breaks into rows when it is wide
  (the Gear's). Fit leaves room for two rows of it while a drawing tool
  is armed, so the view refits when a tool is taken up or put down
  (phase 4's target bar deliberately does not). The Inspector stays
  closed while a drawing tool is armed. The Snapping switch is a row of
  the View menu and the kinds are under *Snap To*, where the spec had
  both under one submenu: a shortcut's key must show in its menu's own
  row. The zoom percentage now goes before the zoom buttons in a
  narrow Viewport; at 1280 wide with an Inspector open the percentage
  is hidden. Icons are all Lucide's (it has an ellipse and a pill); the
  Arc's is `radius`, which may deserve a drawn glyph. No palette token
  was added: the draft is `accent`, the snap marker
  `viewport.shape.hover`. The Construction switch is left for slice 22
  with the rest of construction geometry. The pill's Edit
  nodes, Trim and Measure buttons come with their tools. The diameter
  sign in size labels is `Ø`: the bundled Inter has no `⌀`.
- Slice 10: the selected nodes and the edited shape are the tool's,
  the shape being the app's one selected instance; there is no edit
  session to apply or cancel (upstream's), each edit being its own undo
  step. Breaking an open path makes two shapes only when the shape is
  one path of its own with no linked copy; otherwise the definition
  gains an outline. The context menu rows and the `B` key are the
  window's (slice 17); the app core takes them as `NodeAction`s.
- Slices 11 and 12: a fillet's radius is typed (the bar's corner size),
  not dragged out from the corner as upstream also allows. One size
  serves Fillet, Chamfer and the Rectangle tool's corners. Fillet
  Corners takes one shape at a time: several would need several
  definitions changed in one undo step, which no command does yet.
- Slices 13 and 14: the design's `ReplaceShapes { remove, inherit:
  Option }` became `{ from, keep, inherit: Vec<(shape, index)> }`, so
  an offset of several shapes without their originals hands each one's
  operations to its own result, an offset that keeps its originals is
  still named for them, and its results stand beside them. For an open
  path *Outward* is the left of its direction and *Inward* the right;
  the window is to label them Left and Right.
- Slices 15 and 16: the DXF importer's stitch was not moved to
  `dc-geometry` as the design said. It is 1,400 lines built for files of
  thousands of entities (a grid, a work budget, hatches, duplicates) and
  it moves a joined piece by the gap, where Join bridges the gap; a
  second, small implementation of the same rule serves a selection, and
  the importer and its golden corpus are untouched. Trim treats an
  outline's own end that touches another outline as no crossing.
- Slice 17: *Offset…*, *Fillet Corners…* and *Join…* take their value
  in the tool bar, not in popovers as the specs had them: the preview
  is on the stock where a popover would lie over it, and the bar
  already had fields, choices, Done and a place to say why not. Their
  values are remembered for the session only. Trim is in the Draw
  button's popover, not on the pill: with Edit nodes and Trim both
  there the zoom buttons went at the Viewport's least width. An offset
  of open paths reads *Left* and *Right*; a selection with a closed
  shape in it reads *Outward* and *Inward* for all. The Viewport has no
  context menu yet; the rows the spec gives it are on the tool bar and
  in the Shape menu, and the menu itself comes with slice 18 (cut,
  copy, paste, Make Unique). A closed path drawn with the Line tool is
  still named *Line 1*, which reads oddly beside its square icon in the
  Design Tree; *Path* may be the better name for a closed one.
- Slice 18: the Viewport's context menu on a shape is the Design Tree
  row's menu, one menu in both places, with Cut, Copy, Paste, Make
  Unique and Convert to Path added to it. A hidden shape that is
  selected is copied too (upstream copies only visible ones), and
  pastes visible and unlocked. A paste with no pointer lands 10 mm off
  the originals and 10 mm further each time; what was cut pastes first
  where it was. A pasted shape is never linked (upstream has a
  project-wide copy mode). Tabs and operations are not copied with a
  shape. The copy an Alt-drag leaves is unlocked and labelled *Copy*,
  where Duplicate's is *Duplicate*. Cut and Copy put the shapes' names
  on the system's clipboard as text: egui sends the Paste key on only
  when the system's clipboard holds something, so without it ⌘V would
  do nothing after another app cleared the clipboard; with a file or
  nothing on the system's clipboard ⌘V still does nothing, and Edit ▸
  Paste works. `CopyInstances` has no folder yet: it comes with arrays,
  which file their copies in one.
- Slice 20, done before 19 because arrays need slice 21's folders: a
  locked shape counts toward the box the others align to (upstream
  aligns the unlocked ones among themselves), so a shape can be aligned
  to a locked one. Align and Distribute are a section of Properties,
  under the placement fields, not buttons of the Arrange group in the
  Inspector's header, which has no room for ten more. *To stock* is
  kept for the session only. The undo step is *Align 3 shapes*, without
  the side.
- Slice 21: every command now leaves a folder's rows together in the
  file, so a copy, a paste or a new shape filed in a folder goes after
  the folder's other rows, not at the end of the file (capability 90
  said the end; it is reworded). A file written by hand with a
  folder's rows apart is listed as if they were together, and is put
  so by the next command that adds, deletes or files a shape. Deleting
  a folder deletes its shapes, as the spec says; upstream keeps them.
  A shape can be dragged out of a group (upstream refuses), and a
  group left with no shape goes; a plain folder may stay empty.
  `Ungroup` takes several folders, so shapes of two groups ungroup in
  one step. *Ungroup* on a plain folder dissolves it too; there is no
  way yet to turn a folder into a group or back but to ungroup and
  group again. A marquee that touches one shape of a group takes the
  group. A double-click on a shape of a group selects that shape and
  does not arm Edit nodes. Which folders are folded is not saved.
  Dragging rows does not scroll the tree at its edges.
- Slice 19: Array… takes its values in the tool bar, as Offset… does,
  not in a popover; its button is *Done*, not *Create*. The group an
  array makes holds its source too, so the array moves as one (the
  spec said the copies). A grid's spacing is a gap by default, where
  upstream's is a pitch: copies of a wide shape do not overlap at the
  first values. Upstream's taper of sizes along an array and its array
  along a path are not ported. At most 1000 copies. The bar's count
  field of a circular array is labelled *Count*, its turn choice
  *Copies*. The Inspector is now closed while any tool-bar command
  (Offset…, Fillet Corners…, Join…, Array…) is armed, as it is while
  drawing.
- Slice 22: one palette token is added, `viewport.construction`
  (`#4F63C6` light, `#8EA2FF` dark), as a row of phase 4's
  `experience.md` table, which the theme's test reads; the other
  tokens the design first listed (draft, node, handle, snap) are not
  added, since those are drawn in existing colours. One renderer style,
  not two: selected or under the pointer, construction geometry takes
  the open-path styles, so it stays dashed. The drawing tools' switch
  is a two-way choice (*Off*, *On*), the bar having no switch of its
  own. *Add operation* on a mixed selection makes the operation for
  the shapes that are cut and passes the construction ones by without
  a word; alone, they are refused with the reason. Turning
  Construction off does not put a shape back in the operations it
  left. A new shape drawn while a group's shapes are selected joins the
  group, as capability 90 says of any folder.
- Slice 23, the fonts (functional open question 2, the maintainer's to
  settle): the first proposal is bundled as it stood, Inter Regular and
  Bold, Source Serif 4 Regular and Bold, Roboto Slab Bold, Pacifico,
  Allerta Stencil, each fetched from its publisher on 2026-11-23 and
  unmodified. **Roboto Slab is Apache-2.0, not OFL** as the design
  said; the design now says so. The fonts add about 1.9 MB to a binary.
  **For the maintainer to decide before a release:** the Open Font
  License asks that its text travel with a bundled font; a release
  archive carries `NOTICE` and `THIRD-PARTY-LICENSES`, and neither
  holds the OFL's text (`about.toml` lists crates, not assets). Phase
  4's interface fonts are in the same position. The licence files are
  in the source tree and `NOTICE` names every font and its copyright. Settled in slice 28: every archive now carries `FONT-LICENSES`.
- Slice 23, the layout: the capital height is measured on the font's
  own `H`, with `capHeight` second, so an H is the height asked in a
  script too (the design had them the other way round). Kerning reads
  GPOS pair adjustments as well as the old `kern` table, which none of
  the bundled fonts has. Winding and overlaps are resolved by the
  non-zero fill the SVG importer uses, not by counting containment, so
  a script's joined letters are one outline; the vertices are then the
  boolean's, a few micrometres from the flattened curve. `layout`
  returns the outlines with the widest line's width and the warnings
  (a missing glyph, once a character). A variable font is read at its
  default instance, a collection at its first face; text is laid left
  to right with no shaping, so Arabic, Indic and other scripts that
  need it are out of scope. *Simple Stroke* is ported without its
  italic and condensed variants.
- Slice 24: the words' field is labelled *Words* and the alignment
  *Lines* (the spec had *Text* and *Align*): the tool and the section
  are already headed *Text*, and Properties has an *Align* of its own
  for shapes. A new text is named *Text 1*, not after its words. The
  Font list names a style only when it is not the regular one. A font
  the user adds under the family and style of one already on hand is
  not added twice (the one on hand stands), though its file is still
  copied to the fonts folder. Without a config directory an added font
  lasts until the app is closed. Removing a font from the app is done
  by deleting its file from the folder; there is no button for it. A
  text whose font is missing keeps its Font list open and everything
  else closed. A click on the stock that closes an open list or menu
  is no longer also taken by the armed tool as a point. The words in
  Properties are written when the field is left, not as they are
  typed, so one edit is one undo step.
- Slice 25: *Set scale…* is a mode with the tool bar, as Offset… and
  Array… are, not a popover. Its two points do not snap: they are marks
  in the picture. Opacity can be set on a locked picture (a lock stops
  moves, not looks), and for several pictures at once. A locked picture
  is left out of a marquee as well as a click; phase 4 picked a locked
  picture as it picks a locked shape, and that test now says otherwise.
  A locked *shape* is still picked on the Viewport. Fading a picture
  regenerates no Photo V-Carve: the digest reads the bitmap with its
  opacity left out. **Done** is offered once the distance typed
  differs from the one the points have.
- Slice 26: Measure is in the Draw button's popover, under Trim, with
  its key `K`; the spec had it as a button of the pill, and a sixth
  button there costs the zoom buttons at the Viewport's least width
  (the same reason Trim is in the popover). ΔX and ΔY are signed and
  the angle runs anticlockwise from +X through 360, where PureCutCNC
  shows both without their signs. A tape that is laid is dashed like
  the one in progress (upstream draws it solid). A shape's Length is
  the sum of all its outlines, holes too; its Area leaves holes out. A
  selection being rotated shows no size label, and a dragged picture
  shows one as a shape does. The Draw menu of the menu bar lists the
  drawing tools only: Trim and Measure are reached from the popover
  and, for Measure, `K`.
- Slice 27, the test part: the two slots are an array of two (a linked
  copy at a pitch of 70 mm), not "drawn once and mirrored": a slot is
  its own mirror image, so a mirrored copy in place is the same slot.
  The holes are a Drill with a 5 mm drill; the chapter says to use a
  Bore and the 1/8 end mill where the spindle cannot run slowly enough
  for a twist drill. The lettering is centred with Align ▸ Centre and a
  typed Y, since the Text tool's start is a click. The plate's Profile
  uses the 1/4 end mill and the pockets the 1/8, so the part needs four
  bits. The feeds and speeds in the chapter are starting values for
  plywood of my choosing, as the first chapter's are: **check them
  before the bench cut**.
- Slice 27, acceptance 128: "drawn equals imported" holds byte for byte
  for the drawn part with its sizes removed, which is what S21 claims.
  Through a real SVG file it holds move for move to the last decimal
  place, but not byte for byte: the importer rounds coordinates to a
  millionth of a millimetre, which tips a coordinate on a rounding tie
  (`Y34.608`, `Y34.607`), and is enough to change the order a V-Carve
  takes two equally near strokes in. The lettering is compared as a
  carve (as many moves, the same box, the same cut length). The spec
  says so now. If byte equality through a file matters, the drawing
  tools would have to round a new outline as the importer does; they do
  not today, and a saved project round-trips exactly as it is.
- Slice 27: found while building the part and fixed test-first: an open
  construction line raised `open_profile` ("no operation cuts it").
  Construction-only open paths are no longer reported.
- Slice 27, the book: every drawing chapter's pictures are of the hook
  plate. The chapters reuse the quick start's pictures where one shows
  the thing. The README's status line still names `v0.4.0-alpha.2`: the
  version there is the maintainer's to change when a tag is cut.

- After slice 27, from the CI-only checks: `cargo deny` reported
  `ttf-parser`, which `dc-text` was written on in slice 23, as
  unmaintained (RUSTSEC-2026-0192), and named `skrifa` as the
  alternative. `skrifa` 0.44 was in the app already, through egui's
  text renderer, so `dc-text` was ported to it and `ttf-parser` is out
  of the tree; no advisory is ignored in `deny.toml`. The port is
  like for like (names, glyphs, outlines, advances, `kern` and GPOS
  pair kerning, line metrics): the hook plate's saved project and every
  program it exports are byte for byte what they were before it.
  `docs/rust-guidelines.md` now names `skrifa` as the crate for fonts.
  A font file that is damaged is read by a different parser than
  before, so which damaged files are refused may differ; both refuse
  what is not a font.

## Review feedback, 2026-11-23

The maintainer's first manual test of the drawing tools gave twelve
items. Each is tracked here.

| Item | Kind | Where it stands |
| --- | --- | --- |
| How to make an array | Question | Answered (Arrange ▸ Array…); Array… is now in a shape's right-click menu and, at the maintainer's second word, a button of the tool strip beside Trim |
| Group the drawing tools as Affinity does | Change | Done: the tool strip, with three groups and Text, Trim and Measure as buttons |
| Join open shapes into a closed one | Question | Answered (Shape ▸ Join…; Edit nodes, an end dragged onto an end); at the maintainer's word Join, Weld, Subtract and Intersect are buttons of the strip |
| What the Tab tool on Design is for | Question | Answered; at the maintainer's word the tool left the strip and is **Shape ▸ Tab Tool** and the T key |
| A more prominent division of the tool bar | Change | Done: the view's controls keep the top pill; the strip under it has a rule between each of its four segments |
| Control of snapping | Feature | Exists in View ▸ Snapping and Snap To; now also the strip's Snapping button, with the switch and a tick per kind |
| An origin for an object | Feature | Done, as the maintainer answered: the point a resize, a turn and a flip are made about. `dc_app::origin`, the Origin row in Properties, the mark on the selection |
| Folder and group | Question | Answered, and merged at the maintainer's word: one thing, a **group**, with *Select Together* in its menu; the project file and the code renamed to match (`groups`, `group`, `together`) |
| A tool added in an operation should be chosen | Bug | Done: `Command::AddToolForOperation`, `editor::add_from_library_for` |
| Start and stop points for an operation | Feature | Skipped for now, at the maintainer's word |
| An operation that cuts nothing is too quiet | Bug | Done: `operations::cuts_nothing`, the row's second line, the editor's banner |
| Stretch one way in the UI | Bug | Done: `Grab::Side`, a handle at the middle of each side |

Open, waiting for the maintainer's answer:

- **Start and stop points for an operation** (skipped for now). A Profile has **Start**
  in its Strategy group (*Auto*, *First vertex*, *Near* a typed point).
  Missing: picking the point on the Viewport, a start for the other
  operations (a Pocket's entry, an Engrave's first path), and for an
  open path which end it starts from. A closed outline's cut stops
  where it starts, so "stop" is asked about: an open path's direction,
  or where the bit lifts. This is CAM, phase 4's ground, and wants a
  short spec before it is built.
- **The file's names for a group.** Done at the maintainer's word of
  2026-11-23: the project file writes `groups`, an instance's `group`
  and a group's `together`, with ids `g1`, `g2`, …, and the code's types
  are named to match (`Group`, `GroupId`, `Command::NewGroup`). Format
  29 changed in place, since no release has written it, so a project
  saved by an earlier build of this branch (which wrote `folders`) is
  refused; in such a file, `folders` becomes `groups`, each instance's
  `folder` `group`, and each entry's `group` `together`. X and Y stay
  at the bottom-left corner whatever the origin, also at the
  maintainer's word.

Decisions taken with these, for the maintainer:

- A side handle keeps the opposite side where it is; a corner handle
  still scales about the centre, as phase 4 made it. The two differ.
  Making corners keep the opposite corner would be the consistent
  choice and is a change to phase 4's behaviour, so it is not made.
- A side handle stretches whatever the proportion link says: the link
  is for corners and for typed W and H.
- *Cuts nothing* is the app's reading of a finished result, not a new
  warning code, so `dreamcarve check` prints what it printed.
- With several tools ticked, *Add from library…* adds them all and
  chooses none.
- Every group is made with *Select Together* on, also one made empty
  from the Shapes header; a group that only files shapes is that with
  the tick off. A group now goes with its last shape whether the tick
  is on or off, where a plain folder used to stay empty; one made empty
  stays until it is used. A shape can be a group of one.
- The origin is a preference of the app, not of a shape or of the
  project: it stays as set for whatever is selected next, and is the
  centre again when the app is started. X and Y still read the
  bottom-left corner, so no position in the book or in a saved habit
  changes meaning. A turn or a flip about a point other than the centre
  is labelled *Rotate “name”* or *Mirror “name”* in Undo, without the
  angle that a turn about the centre names.
- The tools are a vertical strip, not a wider top pill: ten buttons and
  three lists do not fit across the Viewport beside the *By operation /
  By depth* pill, and Affinity's tools run down the side. The picture
  the maintainer attached did not arrive, so the layout is from the
  written grouping.
- A click on a group's button arms the tool it holds and opens the
  group's list. The list does not have to be dismissed: a press on the
  Viewport, or a tool's key, goes through.
- The Tab tool stays in the strip, with Trim and Measure, since it
  works on shapes on the Design step; it is not a drawing tool.
- Every picture in the book shows the strip, so all of them were taken
  again.

### Second round, 2026-11-23

Five more items from the maintainer's work with the drawing tools.

| Item | Kind | Where it stands |
| --- | --- | --- |
| Offset geometry | Feature | Existed as Shape ▸ Offset… (Shift+O); now also the strip's **Offset** button, beside Trim |
| Two open shapes drawn end to end should be one closed shape | Bug | Done: a path drawn from or to the end of an open path carries it on, and closes it when the ends meet (capability 90a; `tool::path::carried_on`) |
| Deleting every shape goes back to the start screen | Bug | Done: the start screen is for a project with no step to undo or redo (`steps::untouched`) |
| Snap a line or an object to vertical or horizontal | Feature | Done: the snap kind **Horizontal or vertical** (`SnapKind::Axis`), a dragged node held level with its neighbours, and Shift holding a move level or upright (`handles::straight`). Shift already held a drawn point to 15° steps |
| An arc and a line would not join (2026-11-24) | Bug | Done: they joined end to end, but a line across an arc's two ends was refused, since closing needed three nodes. `Path::closed_up` closes two nodes when a span between them bends; the Line and Arc tools close on the first point the same way |
| A line needs a width | Bug | The maintainer's picture showed it: H 0 was refused. Done: H 0 lays a line level, W 0 stands it upright (`properties::straighten`) |

Decisions taken with these, for the maintainer:

- A drawn path joins only an end it is exactly on (1e-6 mm), which a
  snap to the end gives. There is no switch for it: two lines that
  should stay two can be drawn with the first locked, or broken apart
  again in Edit nodes. If that proves a nuisance it wants a preference.
- The joined shape is the path the drawn one starts on, else the one
  it ends on: its name, place, group and operations. Only paths of the
  same kind join, cut with cut and construction with construction.
- Horizontal or vertical reaches 8 px, not the 14 px of the other
  kinds, and half a grid step while the grid snaps, so a shallow line
  and the next grid line can still be had. It ranks under an endpoint
  and over a point on an outline.
- It is its own switch in `gui.toml` (`[snap] axis`), not a name in
  `kinds`, so preferences saved before it have it on.
- A size of 0 changes the line's outline, and so every linked copy of
  it, since no placement can give a shape no size. It is for one shape
  at a time, an open path of straight pieces.
- The start screen still comes back after *Draw from scratch* and
  Escape with nothing drawn: nothing was done to the project.

### The tester's round, 2026-11-24

A second pair of eyes: an agent playing a hobbyist with years on
VCarve-class software, with the book closed, took a flat-pack table, a
lettered inlay and a photo carve from a blank window to G-code. Its
reviews are kept outside the repository. What came of it:

| Finding | Kind | Where it stands |
| --- | --- | --- |
| Through-cut allowance adds no depth | Change | Done at the maintainer's word: a cut as deep as the stock is a through cut and goes the allowance below it; what is below the stock is taken with the last pass (`dc_project::through_cut_depth_mm`, `Operation::cut_depth_mm`, `passes::pass_z_mm`); a line under Depth says so |
| Messages name the file's fields | Change | Done at the maintainer's word: `warnings::present` calls a field by its label; `present_for_tool` for a tool's own messages |
| "No room for a ramp" on an 80 mm round pocket | Bug | Real, fixed: `entry::Area::line_interval` took the room on one side of a start point on the boundary only, so a ring that started on the upper half of a round wall plunged |
| Rapid (G0) moves below the stock top in V-carve output | Question | By design: safety rule S2 lets a rapid straight down stop 1 mm above the lowest Z a cut of the same operation has already reached at that XY, and the safety pass checks it on the parsed program. It does not hang on pass order: it reads the moves already written |
| The tab request switch turns off after *Place tabs* | Question | By design (ADR-0005): the request becomes placed tabs and is cleared. The button greys out |

Not done, for the maintainer: a simulation (phase 6), a starter tool
library, photo tone controls, plain help for the inlay terms, nesting
(phase 8); Enter after X in a tool's first field commits with Y at its
default; messages still say `null` and name `config.toml` settings.

### Formulas in fields, 2026-11-25

The maintainer's request: a field takes a sum (`1.5 + .375 / 2`) and
holds the result, with no variables and nothing remembered. Done:
`dc_app::formula` (`resolved`, `resolved_decimal`), run before every
number parser of the app (`units::parse`, the tool, project, picture,
shape, rotation and editor fields). PureCutCNC has nothing to port.

Decisions taken, for the maintainer:

- One unit, after the sum. `1 in + 3 mm` is refused, not converted:
  mixing units needs each term's dimension tracked, and a product of
  two lengths would be an area.
- A plain number, `1/4 in` and `1 1/2` go to the file's grammar
  untouched, so they mean what they meant, exactly. In a sum a fraction
  is a division and `1 1/2` a mixed number.
- The result is rounded to nine decimal places, so `0.1 + 0.2` is 0.3.
- No functions (`sqrt`, `sin`), no `^`, no implied multiplication
  (`2(3)`): each is easy to add if wanted.
- While a sum is half typed in a drawing tool's field, the field shows
  the sum's problem until it is finished, as it does for any text
  that is not yet a value.

### Where an Engrave enters and leaves, 2026-11-25

The maintainer's request, needed for a project in hand: an engraved line
entered a quarter along and left three quarters along. Done:
`EngraveOp::enter_fraction` and `leave_fraction` (format 30, `v29_to_v30`),
`dc_cam::engrave::route`, **Enter at** and **Leave at** in the Engrave's
Strategy group. PureCutCNC has nothing to port.

Decisions taken, for the maintainer:

- **Format 30, not a change to 29 in place.** The maintainer has
  projects saved by this branch; a changed 29 would have refused them.
- A point is a fraction of the path's length from its first point, typed
  as a percentage. Not a distance, and not a click on the Viewport: both
  could be added. With several paths in one Engrave it applies to each.
- The route is the shorter of the two that cut the whole open path.
- Every pass takes the same route and lifts between passes; Alternate
  direction is not applied, and not shown, while a point is set.
- A closed path starts where it enters and, with a leave point, runs on
  to it.
- Engrave only. A Profile along an open path (On line, Left, Right) is
  not covered.

### Enter and leave by a distance or a click, 2026-11-25

The maintainer's second request of the day: say where an Engrave enters
and leaves by a distance from either end, or by clicking a point on the
Viewport. Done: `dc_project::PathPoint` and `EngraveOp::{enter, leave}`
(format 31, `v30_to_v31`), `dc_cam::engrave::along_mm`, the warning
`engrave_point_past_end`, and **Enter at** and **Leave at** as choices in
the Engrave's Strategy group, each with its value's rows and, for a
point, the Profile start point's *Pick on the view*. The plaque's Engrave
saved at format 30 with fractions is committed with the programs the
format 30 build exported, and exports them byte for byte at format 31.

Decisions taken, for the maintainer:

- **Format 31, with format 30's two fields replaced**, not kept beside
  new ones: one value per end, `"auto"` or a point, so a file cannot say
  two things at once. `null` became `"auto"`, as `StartAt` writes it.
- "Distance from" read as a length along the path from its first point
  (**From the start**) or back from its last (**From the end**). Not a
  straight-line distance from a point: that is what **Near a point**
  covers. Confirmed by the maintainer on 2026-11-25.
- A distance longer than a path is that path's far end and warns,
  located on the Distance field; it is not refused, since one Engrave's
  paths differ in length and validation does not measure them. A
  distance of 0 is the end itself. Confirmed by the maintainer on
  2026-11-25 ("Warning is fine").
- On a closed path the start is the end, so **From the end** counts back
  round from the start.
- **Near a point** takes each path's own nearest point however far away
  the point is, the first along the path on a tie, as the Profile's start
  point does; no warning for a point far from a path.
- Choosing a mode writes a value at an end of the path: 0 % to enter,
  100 % to leave (which cut as Auto does, but for Alternate direction),
  a distance of 0 (that end itself); a point starts at the stock's
  centre, as the Profile's does.

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 5.1 Drawing

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 81–106, acceptance 107–130, S21, the hook plate), `design.md` (27 slices), ADR-0006; `NOTES.md` folded in; the roadmap's phase 5 brought up to date | done |
| 1 | Format 29: a definition's `shape` (`dc_project::Shape`, eight kinds, data only), `dc_geometry::Path` (nodes with line, arc and cubic spans; exact from a polyline, flattened to one), an instance's `construction` and `folder`, `folders`, a bitmap's `opacity_fraction`; `v28_to_v29`; fixtures `v29/sample.dcv` and `drawing.dcv`; the problems `construction_targeted`, `duplicate_folder_id`, `dangling_folder`, and `out_of_range` at a shape's size or a bitmap's opacity; a path's nodes one per line in a saved file; the quick-start projects and the book's Project file, Images and first-cut chapters at 29. No program changed: a version 28 project exports byte for byte. Gate: 3,449 tests | done |
| 2 | Shape outlines: `dc_geometry::shapes` (rectangle, rounded and chamfered, circle, ellipse, regular polygon to corners or flats, slot) with PureCutCNC's slot, n-gon and canned-rectangle tests ported, and `shapes::gear`, `gearProfile.ts` ported with its tests; every outline counter-clockwise, which upstream's are not as a set; `Shape::outline` and `outline_matches`; the warning `shape_outline_differs`; a gear's sizes checked together as `out_of_range` at the size at fault; the drawing fixture now holds every kind but text. Two properties: an outline is simple and its box is its sizes; a gear is simple and symmetric. Gate: 3,490 tests | done |
| 3 | Paths: `dc_geometry::path` edits a path node by node, `profileEdit.ts` ported with its tests on nodes that each hold the span leaving them: the arc through three points (`bulge_through`), a spline through points (Catmull-Rom, closed or open), split a span, delete a node, break, delete a span, fillet and chamfer a corner with the reason when one is refused, change a span's kind, move a node (an arc beside it rebuilt through its old middle), move a handle (smooth or alone), reshape an arc by its middle or its radius, reverse, close. This is the path geometry of slices 10 to 12 as well; those add the tool. Three properties. Gate: 3,517 tests | done |
| 4 | The tool model and snapping, in the app core: `dc_app::tool` (the tool in use, a drawing tool the Draw button holds, events, the draft the window paints), `dc_app::snap` ported from PureCutCNC's `resolveSketchSnap` with its tests (endpoint, midpoint, centre, intersection, on outline, perpendicular, grid; rank then distance), `dc_geometry::intersect` ported from `segmentIntersection.ts` (brought forward from slice 15: the Intersection snap reads it), a scene shape's centre and construction flag, and the snapping and drawing preferences in `gui.toml` (`[snap]`, `[drawing]`). No window yet: that is slice 9. Gate: 3,550 tests | done |
| 5 | Rectangle, Circle, Ellipse, Polygon and Slot, in the app core: `Command::AddShapes`; the tools' state machine (`dc_app::tool::draw`) with PureCutCNC's click sequences, a drag as two points, `Shift` for a square and `Alt` from the centre, every step typed (the first point as X and Y from the origin, then the sizes; a typed size is pinned and the rest follows the pointer), a refused size marked at its field, the tool bar's prompt and fields as data, the size label, sticky tools, names by the next free number, the Construction switch, the selection's folder; the tools' options in `gui.toml`. Gate: 3,569 tests | done |
| 6 | Line, Arc and Spline, in the app core (`dc_app::tool::path`): one path drawn a point at a time, each span a line, an arc (its end, then a point it passes through, or a typed radius) or a curve (Catmull-Rom through its points); changing among the three keeps the path; a click on the first point closes, `Enter`, Done or a double-click finishes open, `Backspace` takes back a point; the next point by typed length and angle; lines and arcs saved as their outline, a path with a curve as `Shape::Path` and its flattened outline; three points in a line refused with the reason; the tool bar's `can_finish`. Gate: 3,577 tests | done |
| 7 | The Gear tool, in the app core (`dc_app::tool::gear`): the centre, the outside radius, then the sizes in the tool bar (teeth, outside diameter, pressure angle, whole depth, root fillet, crest radius, bore; flanks, root and crest as choices; the module as a note), previewed, until Done; what the gear cannot be said at the field at fault; the bore a second outline; `Shape::from_gear`; `ToolBar::choices` and `note`, `Quantity::Count`. Gate: 3,581 tests | done |
| 8 | Sizes (`dc_app::shape`): the Properties Shape section as data (each kind's sizes as placed, its choices, a gear's module), a typed size or a choice as one `SetOutline` named for it, held and refused as each kind needs, linked copies following; a stretch taken as new sizes by a rectangle, an ellipse, a slot and a circle inside `TransformInstances`, refused by a polygon, a gear, a turned shape and a linked copy; `ConvertToPath`; tabs re-seated on a changed outline. Gate: 3,595 tests | done |
| 9 | The window, drawing: the pill's Draw button and its popover, the tool bar (prompt, choices, typed sizes, Done; wraps into rows), the draft overlay (preview, size labels, snap marker and guide), the tools' keys and Enter, Escape, Backspace through to the tool, the Draw and Shape menus, View ▸ Snapping, Snap To and Grid, *Draw from scratch*, the Properties Shape section, the prompt in the status bar; the Rectangle's corners and corner size and the Polygon's sides and measure in the bar; the Tab tool is `Tool::Tab`; help on every size and choice; a font-coverage test over the strings table; review pictures in `docs/specs/phase-5/review/drawing/`. Gate: 3,608 tests | done |

### 5.2 Editing

| Slice | What | Status |
| --- | --- | --- |
| 10 | Edit nodes, in the app core (`dc_app::tool::node`): the one selected path's nodes, handles and arc middles; select by click, Shift and a box; drag a node, several, a straight side, a curve's handle (Alt for a corner), an arc's middle, each previewed and one undo step, through the instance's inverse transform so linked copies follow; a node's X and Y and an arc's radius typed; add a node (double-click, Ctrl-click), delete nodes or a segment, Break; an end dropped on an end closes the path or joins two shapes; a sized shape converted first; `ReplaceShapes`; `Path::transformed` and `Path::joined`. The window's side is slice 17. Gate: 3,622 tests | done |
| 11 | Fillet and chamfer, in the app core: the Edit nodes bar's *Fillet* and *Chamfer* on the selected corners by its corner size, those that cannot take it left and named; `dc_app::edit::fillet_corners` for Shape ▸ Fillet Corners… | done |
| 12 | Segment kinds, in the app core: the selected segment made straight, an arc or a curve from the bar. Gate: 3,625 tests | done |
| 13 | Offset, in the app core (`dc_app::edit::offset`, `offset_outlines` for the preview): each shape by itself, outward, inward or both, holes the other way, open paths to a side; the original kept or replaced with its operations handed on; `ReplaceShapes` keeps its sources or hands each one's operations and tabs to its own result | done |
| 14 | Weld, Subtract, Intersect, in the app core (`dc_app::edit::boolean`): the selection in order, one path named for the first, placed as it is, with what cut it. Gate: 3,630 tests | done |
| 15 | Trim: `dc_geometry::trim` (the piece between the crossings either side of a point, on the outline's own segments, with itself too) and the Trim tool in the app core (`dc_app::tool::trim`): the piece highlighted, a click taking it out, a sized shape made a path in the same step | done |
| 16 | Join: `dc_geometry::join_open` (chains of open outlines whose ends are within a tolerance, gaps bridged, a chain that meets itself closed) and `dc_app::edit::join`. Gate: 3,641 tests | done |
| 17 | The window, editing: Edit nodes and Trim in the Viewport (nodes, handles, the highlight, the bar's buttons, a double-click to edit), *Offset…*, *Fillet Corners…* and *Join…* as modes of the tool bar (`dc_app::tool::adjust`), the Shape menu, the keys `N`, `B`, `⇧O`, `⌘J`, the editing review pictures. Gate: 3,649 tests | done |

### 5.3 Organising

| Slice | What | Status |
| --- | --- | --- |
| 18 | Make Unique, cut, copy, paste, Alt-drag: `Command::{MakeUnique, CopyInstances, CutInstances, PasteShapes}`, the app's clipboard (`dc_app::clipboard`), Edit ▸ Cut, Copy, Paste and their keys, Shape ▸ Make Unique, the Viewport's context menu (a shape's is its Design Tree row's; the Edit nodes tool's is its bar's buttons), an Alt-drag's linked copy. Gate: 3,660 tests | done |
| 19 | Arrays (done after slice 21: an array files its copies in a group): `dc_app::array` (port: grid and circular), `CopyInstances`' `folder`, the Array mode of the tool bar (`tool::arrays`), Arrange ▸ Array…. Gate: 3,683 tests | done |
| 20 | Align and distribute: `dc_app::align` (port), `arrange::Action::{Align, Distribute}`, `Command::PlaceInstances`, the Align section of Properties, Arrange ▸ Align and Distribute. Gate: 3,667 tests | done |
| 21 | Folders and groups: `Command::{NewFolder, RenameFolder, DeleteFolder, Ungroup, MoveRows}`, folder rows in `dc_app::tree`, group selection on the Viewport, the Design Tree's folder rows with fold, rename, menu and row drag and drop, the Shapes header's New Folder, Arrange ▸ Group and Ungroup (⌘G, ⇧⌘G). Gate: 3,677 tests | done |
| 22 | Construction geometry: `Command::SetConstruction`, the pickers and target editing passing it by, the artwork checks and Fit Stock to Shapes too, the tree's glyph and menu row, the drawing tools' Construction choice, the Properties switch, Shape ▸ Construction, the `viewport.construction` token and style. Gate: 3,690 tests | done |

### 5.4 Text

| Slice | What | Status |
| --- | --- | --- |
| 23 | `dc-text`: `Font` (a file's bytes, or Simple Stroke), `bundled` (seven font files compiled in, with their licences, `fonts/README.md` and `NOTICE`), `layout` (capital height, kerning, alignment, counters as holes, overlaps welded), the ported stroke table; `ttf-parser` added. Gate: 3,702 tests | done |
| 24 | The Text tool and user fonts: `dc_app::fonts` (the fonts on hand), `dc_app::text` (a text's section and edits), the tool (`tool::text`), `Prefs::text` in `gui.toml`, `dreamcarve_shell::fonts` (the user's fonts folder), the bar's words field and Font list, Add font…, a text's Properties, the missing-font state, ⇧T, the text review pictures. Gate: 3,711 tests | done |

### 5.5 Tracing and measuring

| Slice | What | Status |
| --- | --- | --- |
| 25 | Backdrops: `SetBitmapOpacity` and `PlaceBitmap`, `pictures::{opacity, scale_by_two_points}`, the digest without opacity, a locked picture not picked on the Viewport, *Set scale…* (`tool::scale`), the Opacity field and *Set scale…* in Properties, the backdrop review pictures. Gate: 3,716 tests | done |
| 26 | Measure (`tool::measure`: the tape, a shape's length and area, `ToolBar::readings`, `Draft::dashed`), `K`, the popover's row, and the size a selection reads while it is dragged or resized (`handles::size_label`); the Measure review pictures. Gate: 3,722 tests | done |

### 5.6 Release

| Slice | What | Status |
| --- | --- | --- |
| 27 | The hook plate from the quick start's own tables (`design_steps`), through the window (`design_window`, `first_design.rs`) and against the CLI on six post-processors (`gui_equality`); the quick start "Your first design" with 20 pictures; nine user-guide chapters with 8 pictures, and The app, Placing shapes, Images, Overview, Warnings, Porting, Crate map, README, `CONTEXT.md` and the changelog brought up to date; construction-only open paths no longer warned about. Gate: 3,727 tests | done |
| 28 | The release `v0.5.0-alpha.1`, at the maintainer's word on 2026-11-25 after their tests. `FONT-LICENSES` (new, at the root and in every archive through `include` in `dist-workspace.toml`) holds the licence text of each bundled font and icon set, settling the OFL question left from slice 23; `fonts::font_licenses_hold_every_bundled_licence_text` checks it against the licence files in the tree. README and the Installing chapter name `v0.5.0-alpha.1`. The checks CI runs and the local gate does not pass (above). `main` fast-forwarded to `Phase-5` and the tag pushed; `release.yml` builds and publishes from the tag | done |
