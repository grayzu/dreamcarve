# Phase 4: GUI, CAM only — Functional Spec

**Status:** accepted (merged in #460, 2026-11-03); amended by the
screenshot review of #549 (2026-11-10, see "Review redesign" below)
**Mock-ups:** `mockups/` (wireframes referenced from the capabilities below;
`mockups/README.md` lists them and how they are rendered)

## Goal

A hobbyist can do everything the phase 1–3 command line does in a desktop
window, on Windows, macOS and Linux: open or create a project, bring in SVG
and DXF drawings and PNG or JPEG pictures, place them on the stock, set the
stock, origin and heights, pick tools from the library, add and edit
operations of every kind (Profile, Pocket, Drill, Bore, Engrave, V-Carve,
Inlay, Photo V-Carve), see every toolpath over the stock as it is generated,
read the same warnings `check` prints, and export the same programs `export`
writes. No drawing: the shapes come from files, and the app moves, rotates,
scales, mirrors and deletes them but never edits their geometry. That is
phase 5.

The app and the CLI are two shells on one engine. Whatever the window shows
is what the saved `.dcv` says, and the program the window writes is the
program `dreamcarve export` writes for that file. Every safety rule from
phases 1–3 (S1–S19) holds unchanged, and one new rule (S20) says the
window may not bypass them.

Where VCarve Pro and PureCutCNC differ in how a workflow is presented, this
spec follows PureCutCNC's layout (its CAM panel, design tree, export dialog
and canvas are the reference: `src/components/cam/`, `feature-tree/`,
`export/`, `canvas/`) and VCarve's vocabulary, as phases 2 and 3 did for the
operations. What each panel holds follows that reference; how the window
arranges the panels (a rail of five steps, a step panel, an Inspector) is
`redesign.md`'s (capability 50). The GUI toolkit and the architecture are
the design spec's.

## Users and scenarios

- **Sam** opens the app for the first time. The empty window offers two
  paths: *your drawing* (import one, or open a project) or *an example*
  (three pictured cards). Sam imports
  `coaster.svg`: the circle lands on a stock sized round it. They set the
  stock 12 mm thick, pick the 6.35 mm end mill from their library (its
  feeds come with it), select the circle, add an **Outside Profile**, type
  the depth, ask for four tabs, and watch the blue toolpath and its tab
  marks appear over the circle. The Warnings drawer is empty. Export writes
  `coaster.nc`; the status bar says it will take 4 minutes. Sam saves the
  setup sheet beside it and prints it from the browser. Sam never sees a
  JSON file.
- **Sam, later,** opens `plaque.dcv`, the project they built from the CLI
  in phase 2. Every operation is in the list in cut order; the pocket's
  raster lines are visible on the canvas. They drag the Drill above the
  Pocket, switch the Pocket's clearance strategy to Offset, and see only
  that operation's toolpath change. They press Undo twice; both changes
  come back in order. Save writes the same file the CLI reads.
- **Alex** has a LinuxCNC router and a club Fusion library. In the Tool
  Library window they import `club.tools`, read that 23 tools came in and 2
  did not (and why), and drag a 90° V-bit and a 3.175 mm end mill into
  the project. They build the sign: a V-Carve of the rectangle with the
  oval standing as an island, a clearing pass by the end mill, a 2.5 mm
  flat depth, two countersinks, a tabbed outline. The Export dialog is set
  to LinuxCNC; its *Program Details* list three programs in the order to
  run them with what to do before each. Alex's club bits came in with
  inch feeds, so the Tool Library shows inches while the project stays in
  millimetres.
- **Alex** makes the inlay pair from one project. They add an Inlay of the
  two letters, type the depth, glue gap, plug depth and margin, and the
  thickness of the plug board. The Viewport shows the female carve on the
  stock and, beside it, the male board with the design mirrored, the band
  carved round it and the cut-out's tabs. The Warnings drawer carries
  `inlay_male_mirrored` with the mirror line's X, as `check` does. Export
  lists the programs under *Female board* and *Male board*. After the fit
  test Alex changes the glue gap, unticks the female board in the export
  dialog, and re-cuts the male alone.
- **Jo** imports a DXF of six small parts drawn on three layers and takes
  only the `CUT` layer. The parts come in as drawn; Jo selects two, moves
  them off the clamps by dragging, rotates one 90° with the Inspector's
  Rotate button, and duplicates a third as a linked copy. One Outside Profile targets all
  six. The program extents overlay shows where the bit's edge reaches, so
  Jo knows the clamps are clear before leaving the desk.

## Capabilities

Numbering continues from phase 3's capability 49 and acceptance test 61.
Vocabulary is `CONTEXT.md`'s; three terms this phase adds (**Design Tree**,
**Viewport**, **Toolpath Visibility**) are defined in the Documentation
deliverables. "The user can" means in the app, with mouse and keyboard.
Everything the app can change is a field of the `.dcv`; the app has no
project state of its own. Window layout, recent files and the last export
folder are app preferences, kept outside the project.

### The workspace

50. **One window, six regions** (`redesign.md`, "The window"). Every
    region is a card on the window's backdrop, 8 px from its neighbours.
    - A **header** across the top. At its left the menus (File, Edit,
      Arrange, View, Project, Help; 79a). In its centre the project's
      name, with a dot before it while there are unsaved changes. At its
      right Undo and Redo, the generation state while toolpaths
      regenerate (71), and the window's one primary button. It reads
      **Continue** and opens the next step that is not complete; on the
      Export step it reads **Export G-code** and is the export command
      (77), disabled with the reason in its tooltip while an error blocks
      export or the programs are still being built.
    - The **rail** at the left: the five **steps** of the work in their
      fixed order, **Design**, **Stock**, **Tools**, **Operations**,
      **Export**, each a button with a mark of its state (below). Nothing
      is locked: any step opens at any time, from the rail, from View ▸
      *Go to …*, or with Ctrl/⌘ 1 to 5 (54). Pinned to the rail's bottom:
      **Warnings**, with the count of the worst tier found, which opens
      the Warnings drawer (76); **Appearance**; and **Help** (79).
    - The **step panel** beside it shows the current step: its title, one
      line saying what it holds, then its content. Design: *Add drawing*
      and *Add picture* (55, 56) over the **Design Tree** (57). Stock:
      the units, the stock and the origin (60). Tools: the project's
      tools (63). Operations: the operations list (65). Export: the
      whole export (77), top to bottom: a readiness card (the error,
      warning and hint counts, the estimated time, the Z range); the
      findings that block export, while any does; the output settings
      (60, 62), three groups closed until they are wanted, each with its
      values on its header; the programs and the destination; and, once
      the programs are written, the result in their place (77a). The
      Export step has no Inspector, so its panel is wider than the other
      steps', 440 px, and a program's name, tool and time read on one
      line.
    - The **Viewport** in the centre, with its controls floating over it
      (72): the tools, *Fit* and the zoom at its top left, the colour
      mode and *Layers* (73) at its top right, and the cursor's read-out
      at its bottom left.
    - The **Inspector** at the right, there only while the current step
      has something selected that has an editor: on Design the
      **Properties** of the selected shapes or tabs (58) with the Arrange
      group (58a) in its header; on Tools the selected tool's editor
      (63); on Operations the selected operation's (66). Its close button
      deselects, and with nothing selected the Viewport takes the room.
    - A status bar along the bottom (50b), whose warning and error counts
      also open the **Warnings** panel, a drawer over the Viewport's
      bottom: a card on the Viewport's card, which keeps its size under
      it, with a grab handle that drags it taller or shorter.

    A step's state is computed from the project and its findings, never
    stored:

    | Step | Complete when | Needs attention when |
    | --- | --- | --- |
    | Design | The project has a shape or a picture | A finding is about a shape, or an importer's warning about a drawing is still in the Warnings drawer |
    | Stock | The thickness is set and every shape is on the stock | A stock or origin field has a problem, or a shape reaches past the stock |
    | Tools | The project has a tool | A finding is about a tool (one an enabled operation uses lacks a feed, plunge rate or RPM) |
    | Operations | An enabled operation exists | A finding is about an operation |
    | Export | The last export wrote the project as it stands | An error blocks the export of an enabled operation, or an output setting has a problem |

    A step that is neither is **empty** and has no mark; a complete one
    has a check; one that needs attention has a dot in the error colour
    when any of its findings is an error, else in the warning colour.
    Hints never mark a step. A project with no shape, picture or
    operation has no marks at all: its unset thickness becomes the Stock
    step's error once there is something to cut. The status bar's counts
    and the count on the rail's Warnings button follow the same rule, so
    a blank project shows *No problems*; the drawer still lists what
    `check` reports. While toolpaths
    regenerate, the marks of Operations and Export hold until the results
    are back, so they do not blink.

    Selecting an operation by clicking its cut on the Viewport (72), or
    clicking a row of the Warnings drawer (76), opens the step that owns
    what is named: an operation's field in its editor, a shape on Design,
    a tool in the Tools step's Inspector, a stock or output setting in
    its step.

    The **window title** is the project's name without its extension and
    the app's name, `plaque — DreamCarve`, with `*` after the name while
    there are unsaved changes (`plaque* — DreamCarve`) and `Untitled`
    before the first save; the header shows the same name, and no widget
    shows the file's extension. The step panel and the Inspector resize
    by dragging the gap at their inner edge; their widths, and whether
    the step panel shows (View ▸ *Step Panel*), are remembered between
    sessions in the app's preferences file, `gui.toml` beside
    `config.toml` in the user config directory (open question 11). The
    window is dark until the user chooses Light or System (following the
    OS) from View ▸ Appearance or the rail, so both palettes ship (open
    question 9). Minimum usable window: 1280 × 768, which holds the rail,
    the step panel, the Viewport and the Inspector at their minimum
    widths; in a narrower window the Inspector lies over the Viewport's
    right side rather than the window refusing (open question 12).
50a. **Look and feel.** The app is a current desktop application, not a
    dialog-box program (maintainer, 2026-11-02), and it says what to do
    next (`redesign.md`, principle 1): the rail shows which steps are
    done and which need attention, and one accent button moves on.
    Panels edit the project in place: a change applies as it is made and
    is undone from the Edit menu, never through OK/Cancel forms.
    Validation is inline, under the field (60, 66). Modal windows are
    kept for the few things that are modal (open, save, import, export,
    the Tool Library) and use the same panel idiom inside. Rounded cards
    on a dark backdrop, one accent, one icon set, a consistent spacing
    and type scale, and dark and light palettes designed together, so the
    toolpath colours read on both. No bevelled buttons, no tabbed
    property sheets, no nested dialogs, no wizard that blocks. The
    wireframes in `mockups/` show the layout before the redesign and are
    kept for their labels and field names only; the visual design
    (palette, type, icons, spacing, control styling) is `experience.md`'s
    and `redesign.md`'s. The visual review's pictures (acceptance 104) are
    checked against it, and the quick start's screenshots come from the shipped
    build. Opening a project (Open, New, either New Project from, an
    example) selects its first step that is not complete, so an empty
    project opens on Design and a finished one on Export; after that the
    step the user picks stays until the next project is opened.
50b. **Status bar** (`09-preview-legend.svg`; review 2026-11-10;
    `redesign.md`, slice D4). Its cells are quiet pills in two groups, so
    that none moves when another comes or goes; each is shown only when
    it has something to say. From the left:
    - **Units**, `mm` or `in`, a quiet button whose menu switches the
      project's display units (53).
    - **Counts**: errors, warnings and hints, each a glyph and a number,
      opening the Warnings drawer (76). *No problems* when all three are
      zero, and for a blank project (50).
    - **Message**, the last status message (*Saved plaque*, *shape
      “square” is locked*), or, while an operation is selected and nothing newer has
      been said, that operation's carve summary in one line (76a);
      clicking it opens the drawer at that operation. It takes the room
      that is left.

    From the right:
    - **The job**, one cell: while any toolpath is pending, *Generating 2
      of 5…* with a spinner and *Stop*; after Stop, *Stopped* and
      *Regenerate all*, the only place that command lives (71); and once
      every toolpath is current, the **estimate**, the program's total
      time (78).
    - **The pointer**, one cell left of it: a drag's travel, scale or
      angle while one is under way, else the name of the shape under the
      pointer (75).

    The cursor's position, the Z range and the toolpath view are not in
    the status bar: they float on the Viewport (72, 73, 74). Nor is the
    stock size: the Stock step's summary line shows it.
51. **Project lifecycle.** *New* makes an empty project with the phase 1
    defaults (100 × 100 mm stock, thickness unset, origin bottom-left and
    Z zero at the stock top, grblHAL at its latest version, no tools, no
    operations). *New Project from Drawing…* and *New Project from
    Picture…* start a **new** project and do what
    `init` does (capabilities 1, 20, 46): a stock sized round the drawing
    with its margin, the picture or the shapes placed as `init` places
    them, and, for a picture, one Photo V-Carve on it with `init`'s
    defaults. They differ from *Add Drawing to Project…* and *Add Picture
    to Project…* (55, 56), which add to the open project and leave its
    stock alone; the File menu groups the two pairs apart with a divider.
    *New Project from Drawing* adds the shapes and the stock only: no
    placeholder tool and no Profile (open question 1, decided 2026-11-02),
    since the empty Operations and Tools lists say what to do next (79) and
    a tool with unset feeds is a trap the library picker avoids. *Open* reads any `.dcv` the CLI
    reads, migrates it, and lists migration notes and validation
    warnings in the Warnings drawer. *Save* and *Save As…* write through the
    same serializer as the CLI, so a project opened and saved unchanged is
    byte-identical. *Save As…* is the standard save-a-copy: it writes the
    project under a new name and the window then edits that file. *Recent*
    lists the last ten files. Closing the window, quitting, *Open* and
    *New* (any of the three kinds) with unsaved changes show the
    unsaved-changes sheet: *Save changes to "plaque"?*, with **Save**,
    **Don't Save** and **Cancel**; Save on an untitled project asks where
    first, and Cancel on that dialog cancels the whole action. There is no
    autosave in this phase (out of scope). The window title shows the
    project name and the unsaved marker (50). A `.dcv` dropped on the
    window, or passed on the command line, opens.
52. **Undo and redo** (Edit menu, Ctrl/⌘ Z and Shift+Ctrl/⌘ Z) walk the
    snapshot history. Every change to the project is a named command and
    one undo step: *Move “Oval”*, *Set depth of “Outside cut”* (the name
    in curly quotes), *Reorder
    operations*, *Import plaque.dxf*, *Place 4 tabs*. The menu shows the
    name of the step it would undo. Typing in a field is one step when the
    field is left, not one per keystroke; dragging is one step when the
    mouse is released. Selection, zoom, pan, panel sizes and toolpath
    visibility are not project changes and are not undone. An instance's
    `visible` and `locked` flags are project data (they are saved) and are
    undone. Opening a file clears the history.
53. **Units.** The project's display units (`display_units`, mm or inch;
    capability 9) govern every length and feed field and the status bar.
    Fields show the unit after the value and accept the unit strings the
    file accepts (capability 32): `0.5 in` and `1 1/4 in` in a mm project
    convert on entry; a bare number is in the project's units. The stored
    value stays millimetres and the file is unchanged by the display.
    The switch is the first row of the Stock step (**Units**, a `mm` /
    `in` segmented control above the stock's size) and the status bar's units menu
    (50b); both set the same field, convert the display only, and are
    undoable. The Tool Library has its own display units (64), which are
    an app preference, not a project field.
54. **Keyboard.** Standard shortcuts on every platform (⌘ for Ctrl on
    macOS): New, Open, Save, Save As, Import (Ctrl I), Export (Ctrl E),
    Undo, Redo, Select all (Ctrl A), Delete, Duplicate (Ctrl D), Fit view
    (F), Zoom to selection (Shift F), Hide and show selection (H, Shift H),
    Lock and unlock (L), Rename (F2: the selected shape, or the selected
    operation or tool when its list has focus), Rotate 90° clockwise and
    anticlockwise (R, Shift R), Flip horizontal and vertical (Ctrl Shift
    H, Ctrl Shift V), nudge the selection with the arrow keys by 1 mm
    (Shift: 10 mm, Alt: 0.1 mm), Escape clears the selection or closes a
    menu. Every shortcut appears in its menu item and in the Keyboard
    chapter.

### Bringing geometry in

55. **Add a drawing to the project** (`02-import-dialog.svg`; File ▸
    *Add Drawing to Project…*, Ctrl I, the Design step's *Add drawing*, or
    drop an `.svg` or `.dxf` on the window) adds the file's shapes to
    the open project as new feature instances, placed as drawn, with the
    drawing's origin at the stock's bottom-left corner, then selects them so
    the user can move them (open question 2, decided 2026-11-02). The
    sheet shows the file,
    the shapes it holds (closed and open counts), a preview of the drawing
    alone, fitted to the preview box (no stock outline, no origin, no
    placement note: the placement rule is in the chapter, and the selected
    shapes on the Viewport show it), and the importer's
    options: for DXF the units (read from the file, required when it has
    none, as `init --units` is), the join tolerance (default 0.01 mm) and
    a layer list to tick; nothing for SVG. Shapes are named as `init`
    names them (DXF layer names, SVG ids). Import warnings (entities not
    read, open paths skipped by nothing yet, duplicate shapes dropped)
    appear in the Warnings drawer after the import, once, under the import's
    name. One import is one undo step.
56. **Add a picture to the project** (File ▸ *Add Picture to Project…*,
    the Design step's *Add picture*, or drop a `.png`, `.jpg`
    or `.jpeg`) adds a bitmap (capability 46) at the origin, asking for its
    width *or* height in project units with the other following the
    pixels, and reporting a colour-to-grayscale conversion as `image add`
    does. The picture shows on the Viewport as it does on screen, under the
    shapes, at its footprint. A bitmap is a row in the Design Tree with a
    name, visibility and lock, moves like a shape, and does not rotate
    or mirror in this phase (phase 3 decision). It is never stretched,
    since its footprint keeps the picture's proportions: one picture's W
    or H can be retyped in Properties, the other side following the
    pixels as it does at import, about the footprint's centre, and the
    one selected picture's corner handles resize it the same way, its
    proportions kept (slice D6; for the maintainer to confirm with the
    resize itself). It is deleted like a
    shape; a Photo V-Carve that loses its picture says so. A picture is
    selected from its row, or by a click inside its footprint where no
    shape, tab or toolpath takes the click (it is under all of them), the
    topmost where two overlap. Pictures are selected on their own, as
    tabs are: a marquee takes the pictures it touches only where it
    touches no shape, and a picture just added is the selection.
    Properties shows a selected picture's name, its X, Y, W and H (its
    footprint; several pictures' joint box), its size in pixels,
    read-only, its lock and the Photo V-Carves that carve it; X and Y are typed
    for any selection of pictures, W and H for one picture, with the
    proportion link always closed. A move or a resize is one undo step
    named for the picture, a locked picture refuses both, and the Photo
    V-Carve that carves the picture regenerates over its new footprint.
57. **Design Tree** (`01-workspace.svg`, left). One row per feature
    instance, in file order, with: an eye (visible), a padlock (locked),
    a **shape glyph** that says what the shape is (a closed outline, a
    circle, or an **open path**, drawn as an open curve with no word
    beside it; its tooltip says *Open path: Engrave, or a Profile on the
    line, can cut it*), the name, a *linked* badge when the definition is
    shared with another row, and a count of the operations targeting it
    (hover lists them). The word "open" does not appear on the row. A
    shape is renamed by double-clicking its name, by F2, or by *Rename*
    in its context menu; all three edit the name in place (one undo
    step). Below the shapes, a
    **Tabs** section lists placed tabs by the outline they sit on, and a
    **Pictures** section lists bitmaps; an empty Tabs or Pictures section
    shows its header only, without a *None* row. Clicking a row selects it
    on the Viewport and vice versa; Shift and Ctrl extend the selection; the
    section header's eye and padlock act on every row in it. The padlock's
    tooltip says what a lock does: *Locked: cannot be moved, resized,
    rotated or flipped. Operations still cut it.* While an operation is
    selected its targets are marked in the tree (67). The context
    menu offers Rename, Hide/Show, Lock/Unlock, Duplicate, Delete, Zoom to,
    *Add operation ▸* with the kinds the selection can take, and, while an
    operation is selected, *Add to "Panel"* or *Remove from "Panel"*. Rows
    cannot be reordered or grouped in this phase (folders are phase 5).
58. **Placement** (`01-workspace.svg`, Properties). With one or more
    instances selected on the Design step, the Inspector's Properties show, in project units, the
    selection's **X** and **Y** (bottom-left of its bounding box), and
    **W** and **H** with a **proportion link** to their right: one
    chain glyph spanning both rows, linked (the default) or broken.
    Typing X or Y moves the selection; typing W or H scales it about its
    bounding-box centre, the other following while the link is closed; a
    selection of several moves as one. Rotation and mirroring are not
    properties of a shape: they are actions on the selection (58a). On
    the Viewport the selection drags to move, its corner handles
    scale, its rotation handle rotates, and the arrow keys nudge (54).
    Every transform is a change to the instances' `transform`; the
    definitions are never touched, so a linked copy's sibling does not
    move. A locked instance shows its handles greyed and refuses every
    transform, and the status bar says why; the Properties of a
    locked selection show their fields read-only with the line *Locked:
    cannot be moved or resized. Operations still cut it.* and an
    *Unlock* button. Lock never removes a shape from an operation and
    never stops it being selected. *Fit stock to shapes* (61) is in the
    Project menu. No snapping, no alignment and no distribution: phase 5.
58a. **Arrange** (review 2026-11-10). The Arrange group, in the header
    of the Inspector's Properties (50), acts on the selection: **Rotate** (a split
    button: the button turns the selection 90° clockwise; its chevron
    opens a popover with an **Angle** field, positive anticlockwise as
    the Y-up file is, and a *Rotate* button, Enter applying it), **Flip
    horizontal** and **Flip vertical** (a scale of −1 about the
    bounding-box centre). The **Arrange** menu holds the same commands
    with their shortcuts (54), *Rotate 90° anticlockwise*, *Rotate…* (the
    angle popover), and *Centre on stock* (61). Each is one undo step named for the selection
    (*Rotate “square” 90° clockwise*). The group is disabled, with the reason in
    its tooltip, when every selected instance is locked, and the Arrange
    menu's rows when nothing is selected; with some locked, the unlocked ones turn and the status bar
    names the rest. A picture cannot rotate or flip (56), so a selection
    of pictures only disables the group with that reason.
59. **Duplicate and delete.** *Duplicate* makes a linked copy (CONTEXT:
    Linked Copy) offset by 10 mm, named `<name> (2)`, sharing the
    definition, so phase 5's shape editing will edit both. *Delete* removes
    the instance from every operation's targets as well as from the tree;
    when an operation is left with no targets the Warnings drawer says so
    (validation's `operation_has_no_targets`: an error, so its row is the
    message without the code, as `check` prints an error), and the operation stays, disabled-looking
    but enabled, until the user fixes or deletes it. A definition no
    instance uses is dropped from the file on save. Placed tabs on a
    deleted instance go with it.

### Stock, origin and output setup

60. **Project settings** (`03-stock-origin.svg`). The project's own
    settings are in the two steps that own them (50). The **Stock** step,
    which Project ▸ Project Settings… opens: **Units** first (53); the
    stock's width, height and thickness (marked required until set (66b);
    export refuses without it, as today) with *Fit to shapes* (61); and
    **Origin**. The **Export** step, under its readiness card:
    **Heights**, **Tolerances** and the **Post-processor** (62). A
    project whose thickness is unset under a drawing opens on the Stock
    step, which the rail marks with an error. **Origin**: XY at one of the four
    corners or the centre, or
    custom X/Y, and Z zero at the stock top or bottom. Changing XY zero
    moves the origin marker (the `0,0` crosshair and the axis lines
    through it) to the new point in the same frame; the shapes, the stock
    and the toolpaths stay where they are on screen, because the origin
    moves the program's coordinates, not the part. The Viewport's
    read-out counts from the new point (the cursor is not in the status
    bar, 50b), and the Z-zero choice relabels
    the depth legend (73). **Heights** (capability 33): clearance, tool-change machine Z (only
    offered when the post-processor documents `G53`, greyed with the reason
    otherwise), and park point, which can be typed or clicked on the
    Viewport. **Tolerances**, folded away: flatten tolerance and arc-fit
    tolerance (off switch for `null`). Each field validates as the file
    does and shows the validation message under itself.
61. **Fit stock to shapes** sizes the stock to the visible shapes plus a
    margin the user types (default `init`'s), keeping the origin preset;
    shapes do not move relative to each other. **Centre on stock** moves
    the selection so its bounding box is centred on the stock.
62. **Post-processor** (on the Export step; `03-stock-origin.svg`).
    A list of the six bundled definitions with their latest version and a
    note when the project is on an older version. Picking another keeps
    the user's settings the new one can use, as `export --post` does
    (capability 27), and says which were reset. The settings below it are
    the project's post-processor settings (capability 2): file extension,
    spindle control (automatic or manual), spin-up dwell, tool-change mode
    (one program per tool, or one program with `M6`: offered only on a
    post-processor with `M6`), decimal places for mm and inch, and canned
    cycles (offered only where the definition has them). Nothing about the
    dialect itself is editable: the project names a bundled definition
    (ADR-0004).

### Tools

63. **Project tools** (`16-tools-tab.svg`; the Tools step). The
    tools the project embeds, one row each with type icon, name, diameter
    (the **cutting diameter** for every type: 6.35 mm for a 1/4 in
    tapered ball nose, never its tip), a tapered ball nose's tip, the
    angle labelled for what it is (a V-bit's included, `90°`; a tapered
    ball nose's per side, `6.7°/side`, since the two unlabelled side by
    side are the mix-up phase 3's research warns of), number, and a red
    dot when the tool lacks a value an operation needs: a feed, plunge
    rate or RPM, or a tapered ball nose's tip, taper or flute length. No
    tool is selected until a row is clicked or a tool is added. Selecting
    a row edits it in the Inspector, in place
    (selecting *is* editing; there is no Edit button): name, type
    (end mill, ball nose, V-bit with included angle, engraver, drill,
    tapered ball nose with tip diameter, taper per side and flute length;
    its diameter is the cutting diameter),
    diameter, flutes, feed, plunge rate, RPM, step-down, stepover, tool
    number, vendor and product id. A type's own fields follow the type:
    a V-bit's *Angle*; a tapered ball nose's *Tip diameter*, *Taper per
    side* and *Flute length*. *Diameter* keeps its label for every type.
    Choosing *Tapered ball nose* writes the cutter with none of its three
    values set, one undo step, and each value typed is one more; nothing
    is invented, as no V-bit angle is (a V-bit's switch is held until its
    angle is typed because the file requires one; a tapered ball nose's
    values may be `null` in the file, so nothing is held). Feed, plunge
    and RPM fields are empty,
    never prefilled: the app invents no feed (principle 5). An unset feed,
    plunge rate or RPM, and an unset tip, taper or flute length of a
    tapered ball nose, shows the required-field look (66b), not a
    paragraph under the field. The list's actions, in this order:
    - **New tool** makes a tool from scratch, for a bit that is in no
      library: an end mill named `tool<n>`, where `<n>` is the number the
      tool takes (its position in the list, or the next free number when
      that name is taken), with `init`'s placeholder geometry (6.35 mm, 2
      flutes, the file needs a diameter) and no feed, plunge rate, RPM,
      step-down or stepover. It is selected with its name field focused
      and the name selected, so typing renames it; Tab moves to Type, then
      Diameter. One undo step, *New tool "tool4"*.
    - **Add from library…** opens the picker (64).
    - **Duplicate** copies the selected tool below itself as `<name> (2)`.
    - **Delete** removes it; it is refused while an operation uses the
      tool (the button disables and its tooltip names the operations).
    - **Tool Library…**, at the step panel's foot, opens the library
      (64), as Project ▸ Tool Library… does.

    *Save to library* (in the editor's footer) copies a project tool into
    the library. A tool used by no operation shows an `unused_tool` hint,
    as `check` prints. *Duplicate* and *Delete* name the same actions in
    every list in the app (Operations, Tools, the Tool Library), and *New
    tool* is the same command on the Tools step and in the library;
    *Remove* never labels a deletion (it is kept only for taking a shape
    out of an operation's targets, which deletes nothing).
64. **Tool Library** (`04-tool-library.svg`; Project ▸ Tool Library…, and
    the picker from 63 and from an operation's tool field). A sheet over
    the library in the user's config directory (capability 21), in two
    panes. The left pane: a search box (name, vendor, product, diameter),
    a type filter, and the library's tools **grouped by vendor** (one
    collapsible group header per vendor, `IDC Woodcraft · 6`, sorted by
    name, with *No vendor* last; a search hides groups with no match),
    each row with name, type, diameter, tip, angle, flutes, feed, plunge,
    RPM and product; vendor is the group, not a column. The Ø column is
    the **cutting diameter** for every type (6.35 for a 1/4 in tapered
    ball nose); a tapered ball nose's tip has the column of its own, and
    the search reads it with the diameter; the Angle column says which
    angle it holds, a V-bit's included (`90°`) or a tapered ball nose's
    per side (`6.7°/side`). A value the tool still needs (a rate, a
    tapered ball nose's tip or taper) is a red dash in its cell, never a
    value. Rows already in the
    project are marked. The right pane edits the selected tool in place
    (64a). The header holds the library's **display units**, a `mm` /
    `in` segmented control separate from the project's (53): an app
    preference kept in `gui.toml`, defaulting to the open project's
    units, changing only what the sheet shows; the library file stays in
    millimetres. Actions: **New tool**, **Duplicate** and **Delete** over
    the list (Delete asks once, in place, since the library is outside
    project history, and never touches a project's copy); **Import…** and
    **Export library…** at the right of the header; **Add to project**,
    the primary button, copies the ticked rows into
    the project under new ids, as `tools add` does, and closes the sheet,
    the status bar saying what was added (a refusal keeps the sheet open
    with its reason); projects keep their
    copies and a library change never alters an open project (ADR-0002).
    *Export library…* writes the whole library as one DreamCarve JSON
    file, to keep a backup or to carry the library to another computer;
    its tooltip says so. *Import…* reads a Fusion `.tools` or `.json` or
    a DreamCarve library JSON and shows the review of 64b before anything
    is written. An empty library shows its sentence and the two ways to
    fill it, *New tool* and *Import…*.
64a. **Editing a library tool** (review 2026-11-10; was phase 5).
    Selecting a library row shows its fields in the right pane, the same
    fields, order, units and required-field look as a project tool's
    editor (63), in the library's display units: a tapered ball nose's
    tip diameter, taper per side and flute length among them, each marked
    required while unset (an unset value is not a fault in the library,
    so the file is written; the mark says what an operation will need).
    A field commits on leave
    and writes the library file at once, through the same staged write
    the CLI uses; a write that fails leaves the field marked with the
    reason. Library edits are not project changes: they are not in the
    Edit menu's history, and they never change a project's copy of the
    tool. While the sheet is open, *Revert changes* in the pane's footer
    puts the selected tool back as it was when the sheet opened. *New
    tool* here makes the same `tool<n>` placeholder as 63, numbered within
    the library. *Duplicate* copies a tool as `<name> (2)` with a new
    library id and no `source`, so a later import with *Replace* never
    overwrites the copy. Editing a tool keeps its `source`, so the import
    review (64b) shows it as changed when its file is imported again.
64b. **Importing into the library** (review 2026-11-10). After the file
    is chosen the sheet's panes are replaced in place (never a second
    sheet) by the **import review**: the file's name, then the counts
    `tools import` reports, **New**, **Already in your library** (the same
    tool), **Changed** (a library tool came from the same source and the
    file's entry now differs) and **Skipped** (with the reason each is
    skipped), then the incoming tools in the same grouped table with a
    status column. When any are *Changed*, a segmented control chooses
    **Keep mine** (the default: the file's version is added beside yours
    under a suffixed name, as `tools import` does) or **Replace with
    imported** (`tools import --replace`: yours is overwritten in place,
    keeping its id); the counts and statuses update as it changes. The
    control is absent when nothing is *Changed*, since then it changes
    nothing. This is where *Replace existing* lives; the library table
    has no such switch. The footer has *Back* and the primary *Import 21 tools*
    (its count follows the choice). Import writes the library and returns
    to the table with a banner, *Imported 21 tools from club.tools. 2
    skipped.*, whose *Details* reopens the review read-only. The review is
    the report `tools import` prints, computed without writing. A tapered
    ball nose that comes in with values unset is imported and listed with
    them named, *Still to set: Taper per side, Flute length* (the
    report's own note names the same fields as the file does, `set
    cutter.tapered_ball_nose.taper_angle_deg, …`); a Fusion tapered mill
    with no shaft diameter, which is the cutting diameter, or with a flat
    or bull tip is in the skipped list with that reason, never imported
    with a placeholder.

### Operations

65. **Operations list** (`01-workspace.svg`; the Operations step).
    The project's operations in cut order, one row each: a drag grip, an
    enable checkbox, the kind's icon, the name, the tool's name, an eye for
    the operation's toolpath visibility (73), and a status cell: a spinner
    while generating, a warning count, a red mark for an error, nothing
    when clean. Drag to reorder (one undo step); the list order is the cut
    order (capability 10) and nothing regroups it. *Add* opens a menu of
    the eight kinds, the kinds the current selection can take listed
    first and the rest below a divider with the reason (an open path cannot
    take a Pocket; a Photo V-Carve needs a picture). A Photo V-Carve
    carves a picture, not the selected shapes: it is offered whenever
    there is a picture for it, the selected one or, with none selected,
    the project's only one; with several pictures and not exactly one
    selected, its reason says to select the picture to carve. A Pictures
    row's *Add operation ▸* offers it for that picture. The new operation
    targets the selection, takes the project's first tool whose type fits
    (a V-bit for V-Carve, Inlay and Photo V-Carve; a drill or end mill for
    Drill) or none, is named `<Kind> <n>`, and gets the CLI's defaults for
    the kind (the chapter's JSON block: `init`'s Profile, the plaque's
    Pocket, and so on). *Duplicate* copies an operation below itself with
    its targets; *Delete* removes it (placed tabs stay: they are the
    part's, ADR-0005).
66. **Operation editor** (`05-operation-profile.svg`,
    `06-operation-vcarve.svg`, `07-operation-fields.svg`; reworked by the
    review, 2026-11-10). The editor's **header** is the operation's kind
    glyph, its **name**, editable in place (click the name, double-click
    the row, or F2; Enter commits, Escape reverts, one undo step), and the
    kind in secondary text (`Panel · Pocket`). There is no Identity group:
    the name lives in the header and *enabled* only in the list's
    checkbox (65). Below the header the editor scrolls, and holds, in
    this order:
    - **Setup** (66a), always open and never collapsible: the settings
      every operation of the kind needs before it can cut, so the user
      sets them without scrolling.
    - The other groups in the order **Depth**, **Strategy**, **Entry and
      leads**, **Corners and tabs**, **Order** and **Heights**, each a
      collapsible section holding the fields of 68 that Setup does not.
      A group with no field left for the kind is not shown. Groups are
      **closed by default**, and a closed group's header shows a one-line
      summary of its values in secondary text (`Strategy · Raster 0°,
      climb, wall last`; `Heights · Safe Z 5 mm`), so the whole operation
      reads on one screen and nothing is out of sight without a sign of
      it; the open state of each group is remembered per kind. A group
      holding a located error or warning opens by itself and its header
      carries the glyph and count when closed.

    The editor shows every setting of the kind's record (68): nothing is
    hidden and nothing is added; the review moved fields, it did not drop
    them. Every field shows its
    unit, validates on leave, shows the file's validation message under
    itself (or, for an unset required value, the required-field look,
    66b), and shows a short help line (the chapter's one-sentence
    definition) on hover of its label. A field whose file value is `null`
    and whose default comes from the tool (step-down, stepover, pitch)
    shows that default as grey placeholder text, `tool's: 3 mm`, and clears
    back to it with its ⟲ button. Warnings and errors that `check` locates
    at a field (capability 25) are shown under that field as well as in the
    Warnings drawer. The editor never offers a setting the generator ignores
    for that kind, and never hides one it reads. The carve summary `check`
    prints for an operation (*check says…*) is not in the editor: it is a
    note in the Warnings drawer and the status bar (76a).
66a. **Setup, per kind.** The first rows of every editor, in this order.
    Targets and Tool come first for every kind; the rest are the fields
    that decide where the bit goes and how deep, the ones a new operation
    of the kind cannot be right without:

    | Kind | Setup rows after Targets and Tool |
    | --- | --- |
    | Profile | Side, Depth, Tabs (none, a count, or distances; its sizes stay in Corners and tabs) |
    | Pocket | Depth, Clearance strategy (Offset or Raster) |
    | Drill | Cycle, Depth (a countersink's derived depth and its mouth diameter in its place) |
    | Bore | Depth |
    | Engrave | Depth |
    | V-Carve | Flat depth (off, or the depth), Clearing passes (the list of 70) |
    | Inlay | Inlay depth, Glue gap, Plug depth, Male board thickness |
    | Photo V-Carve | (Targets is the Picture) Max depth |

    A Setup field keeps its file name, label, unit, help and validation;
    it is the same row as in 68, moved. The design spec's field table
    marks which rows are Setup rows, and acceptance 73 still checks every
    field of 68 is present exactly once.
66b. **Required values.** A value the file needs before export (a
    tool's feed, plunge rate and RPM; a tapered ball nose's tip diameter,
    taper and flute length; the stock thickness; an Inlay's male
    board thickness) shows, while unset: the field's error outline, the
    `circle-x` glyph inside the field before the unit, and *Required* as
    its placeholder text. The file's message (*tools[t1].feed_mm_min is
    not set…*, *tools[t3].cutter.tapered_ball_nose.flute_length_mm is not
    set…*) is the field's tooltip and a row in the Warnings drawer; it
    is not written under the field, and no dialog asks for it. A tool's
    values are marked from `Tool::unset_values()`, in the tool editor (63)
    and the library's pane (64a) alike, from the moment the tool is made;
    the drawer lists them once an enabled operation uses the tool, as
    `check` does. A tool's `diameter_mm` is always in the file, so it is
    never marked. Every other validation error (a
    value out of range, a depth past the thickness) keeps its message
    under the field, as 66 says.
67. **Targets** (`15-targets.svg`; reworked by the review, 2026-11-10).
    Targets are shown on the drawing, not as a list in the editor.
    - **Highlight.** While an operation is selected, its targets are
      marked where the shapes are: on the Viewport each target's outline
      draws 2 px in the operation's colour and every other shape fades to
      40 %; in the Design Tree each target row has a bar in the
      operation's colour on its left edge. Hovering an operation's row in
      the list shows the same marks as a preview.
    - **Setup's Targets row** is one line: a target glyph, the count and
      the first names (`2 shapes · pocket, island`, truncated, all names;
      one shape reads as its name alone, `outline`, which the row has
      room for whole,
      in its tooltip), a warning glyph when a target carries one, and an
      **Edit** button.
    - **Editing targets.** *Edit* (or double-click on the Targets row)
      enters **target editing**: a bar across the top of the Viewport
      says *Editing targets of "Panel": click shapes to add or remove
      them*, with *Done* (Enter) and *Cancel* (Escape). While it is on,
      clicking a shape on the Viewport or a row in the Design Tree adds it
      to or removes it from the targets (marquee adds every shape it
      touches; Alt-marquee removes), tree rows show a checkbox in place of
      their eye, and nothing else can be selected or moved. *Done* commits
      the new target list as one undo step (*Set targets of "Panel"*);
      *Cancel* leaves the operation as it was. The highlight follows each
      click, so the user sees the result as they go.
    - **Shortcuts.** With shapes selected, the Targets row's menu (its
      `⋯` button) offers *Use selection* (replace) and *Add selection*;
      the Design Tree's context menu offers *Add to "Panel"* and *Remove
      from "Panel"* (57).

    A target the kind cannot cut
    (an open path for a Pocket, a non-circle for a Drill) is accepted and
    carries the warning `check` gives it (`side_needs_open_path`,
    `drill_target_not_circle`), as a hand-edited file does. A Photo
    V-Carve's single target is a picture: its Setup row is a **Picture**
    dropdown of the project's pictures, not target editing.
68. **Fields per kind.** The editor exposes every setting of the kind's
    record in the project file and nothing else. The table is the
    checklist for acceptance test 73; names are the file's, labels are the
    chapters'.

    | Kind | Fields, by group |
    | --- | --- |
    | Profile | Depth: `depth_mm`, `step_down_mm` (tool's), `through_cut_allowance_mm`. Strategy: `side` (Inside, Outside, On line, Left, Right), `direction`, `start` (Auto, First vertex, Near a point: typed or clicked), `last_pass` (off, or `stock_to_leave_mm`, `thickness_mm`, `reverse`). Entry and leads: `entry` (Plunge; Ramp with `angle_deg`; Helix with `diameter_fraction` and `angle_deg`), `lead` (off, or `radius_mm`, `sweep_deg`, `lead_out`, `overcut_mm`). Corners and tabs: `corner_relief` (None, Dogbone, T-bone), `tabs` request (69). Order: `machining_order`. Heights: `safe_z_mm`. |
    | Pocket | Depth: `depth_mm`, `step_down_mm` (tool's), `stock_to_leave_mm`, `through_cut_allowance_mm`. Strategy: `strategy` (Offset; Raster with `angle_deg`), `wall_pass` (Last, First, None), `step_over_fraction` (tool's, shown as a percentage), `direction`, `rest_of` (an earlier Pocket, or none). Entry: `entry`. Corners: `corner_relief`. Order: `machining_order`. Heights: `safe_z_mm`. |
    | Drill | Depth: `depth_mm`, `retract_mm`, `through_cut_allowance_mm`. Strategy: `cycle` (Simple; Peck with `peck_depth_mm` (tool's) and `retract` Full or Chip break with `gap_mm`; Countersink with `diameter_mm`, `start_depth_mm`, `start_plane_cleared_externally`), `dwell_s`, `allow_tool_wider_than_hole`. Heights: `safe_z_mm`. |
    | Bore | Depth: `depth_mm`, `pitch_mm` (tool's step-down), `stock_to_leave_mm`, `through_cut_allowance_mm`. Strategy: `direction`. Heights: `safe_z_mm`. |
    | Engrave | Depth: `depth_mm`, `step_down_mm` (tool's), `through_cut_allowance_mm`. Strategy: `alternate_direction`. Order: `machining_order`. Heights: `safe_z_mm`. |
    | V-Carve | Depth: `flat_depth_mm` (off for `null`), `start_depth_mm`, `start_plane_cleared_externally`, `through_cut_allowance_mm`. Strategy: `strategy` (Medial; Offset with `step_over_mm` and `direction`), `step_over_fraction`, `clearing` (70). Order: `machining_order`. Heights: `safe_z_mm`. |
    | Inlay (format 26; one file since #498) | Depth, one set for both boards (68a): `depth_mm`, `glue_gap_mm`, `plug_depth_mm`, `boundary` (none, or the closed shapes that draw the male band's edge; phase 3's 49f), `margin_mm`, `outline_rounding_mm` (off for `null`), `through_cut_allowance_mm`. Male board: `male_board.width_mm` and `height_mm` (the stock's for `null`), `male_board.thickness_mm`. Clearing: `clearing` (70), `step_over_fraction`. Cut-out: off, or `tool`, `tabs` request, `through_cut_allowance_mm`. Order: `machining_order`. Heights: `safe_z_mm`. |
    | Photo V-Carve | Targets: `image`. Depth: `max_depth_mm`, `start_depth_mm`, `start_plane_cleared_externally`, `invert`, `through_cut_allowance_mm`. Strategy: `strategy` (Raster, Hatch), `line_angle_deg`, `line_spacing_fraction` (as a percentage), `sample_mm`. Heights: `safe_z_mm`, `stepover_retract_mm`. |

    Photo V-Carve lands with phase 3.5 (format v27); its editor is a slice
    of this phase that waits for it, and nothing else in the phase does
    (open question 10, decided 2026-11-02).

    **The carving kinds' tool pickers** (phase 3.6, #685). A V-Carve's
    and an Inlay's Tool row offers the project's V-bits and its tapered
    ball noses; a Photo V-Carve's offers the V-bits alone, since its
    depth follows a V-bit's angle. The picker offers what validation
    takes as the kind's carving bit (`v_carve_needs_v_bit`), so the two
    cannot differ. A tapered ball nose with a value still unset is
    offered, as a tool with no feed is, and its fields say what it needs
    (66b). Under the picker a V-bit shows its cone height and a tapered
    ball nose its cutting height, the lower of its flutes and where its
    cone reaches the cutting diameter (`cutting height 19.05 mm`). Both
    read to a hundredth of a millimetre or a thou, as every length the
    app shows and does not take from a field does, and are rounded
    down, so a depth typed from the line is within the limit.
68a. **Inlay: both boards from one project** (`06-operation-vcarve.svg`;
    maintainer decision 2026-11-02, engine and format change #498, which
    lands before this editor's slice). An inlay is one design cut into two
    boards from one set of numbers (capability 45), and since #498 one
    project holds both: the female carve on the stock, and the male on the
    Inlay's own **male board**, whose width and height default to the
    stock's and whose thickness is its own. There is no side to choose.
    The editor shows the Inlay's values once: inlay depth, glue gap, plug
    depth, margin, outline rounding, through-cut allowance; then the
    **Male board** group: width and height (placeholder *same as stock*),
    thickness (required for the male to export; it sits in Setup (66a)
    and shows the required-field look (66b) until it is set, with
    `inlay_male_board_unset` in the drawer), and the derived base under the band,
    red below 6 mm (`inlay_plug_base_thin`). Under Depth a read-only line
    shows what the bench needs to see: the male's start plane, its band
    floor, and the flank's engagement against the V-bit's cone height, red
    when it exceeds it (phase 3's 90° bit caps it at 3.175 mm). Cut with
    a tapered ball nose (phase 3.6, #685), the engagement is compared
    with the bit's cutting height, and a third line is the **seat line**
    with `check`'s figures: `seat: moves 0.55 mm per 0.5° of taper,
    0.76 mm per 0.05 mm of tip radius; test-fit before gluing`, in
    millimetres whatever the display units, as its steps are. The male's
    band ends where the design grown by the margin ends, or at a **drawn
    boundary** (phase 3's capability 49f): the **Boundary** row, in Depth
    before the margin and the rounding it replaces, is a dropdown of
    *None (use the band margin)* and the project's shapes that can be
    one, which are those validation accepts: every outline closed, and
    not one of the Inlay's own targets. Choosing a shape makes it the
    boundary, one undo step; with two or more such shapes selected on the
    drawing the dropdown also offers *Selected shapes*, for a band drawn
    by several. The row writes `boundary` and nothing else: a margin or a
    rounding still set shows `inlay_margin_ignored` under itself until
    the user clears it, as a hand-edited file does, and the boundary's
    own errors (`inlay_boundary_crosses_design` and the rest) show under
    the row. A shape that is deleted leaves the list. The Viewport draws
    the boundary dashed on the male board, mirrored with the design, as
    `--preview` does. One tool,
    one clearing list and one cut-out serve both boards, so the same bit
    angle on both sides is enforced by construction. The Viewport draws the
    male board beside the stock (72). Export writes both boards' programs
    (77); re-cutting the male alone after a fit adjustment is an export
    choice, not a project setting.
69. **Tabs** (`08-tabs-on-canvas.svg`). A Profile's **Tab request** is
    edited in its Corners and tabs group: none, a count, or distances along
    each outline; width, height, shape (rect or smooth), on holes, avoid
    corners. *Place tabs* runs `tabs place` for that operation: the request
    becomes **placed tabs** on the part's outlines (ADR-0005), drawn on the
    Viewport as marks the tab's width across the toolpath and listed under
    Tabs in the Design Tree. A placed tab drags along its outline (it stays
    on the outline; its point is in definition space, so it follows its
    instance), has width, height and shape in the Inspector's Properties, and
    deletes with Delete. The **Tab tool**, floating at the Viewport's top left, adds a tab where
    the user clicks an outline, with the last-used size. Every enabled
    Profile that cuts that outline rises over it; `tab_not_crossed`,
    `tab_on_hole` and the other tab codes appear as `check` gives them, and
    the Viewport draws a tab no cut crosses hollow. A disabled Profile's
    request is kept until the user places it, as the CLI keeps it. The
    saved file holds placed tabs only, as `tabs place` leaves it.
70. **Clearing passes** (`06-operation-vcarve.svg`). A V-Carve's or an
    Inlay's `clearing` is an ordered list of passes, each with its tool
    (an end mill from the project tools), clearance strategy (Offset or
    Raster with angle), direction and entry. *Add pass* appends one with
    the next smaller end mill; passes reorder by drag; the list enforces
    the file's rule that passes descend in diameter and shows
    `clearing_not_descending` otherwise. The passes' floor rings draw in
    the Viewport in the operation's colour, lighter than the V-bit's lines.
71. **Generation** happens by itself. Every change that affects an
    operation's toolpath regenerates it (and only it, where the engine's
    inputs allow; the design spec owns what counts) on a worker, a short
    moment after the change settles, never blocking the window. While an
    operation regenerates its row shows a spinner and its old toolpath
    stays on the Viewport, faded; when it finishes the new toolpath
    replaces it. A generation error (a tool with no feed, a missing
    thickness) marks the row red and lists in the Warnings drawer; the
    operation draws nothing. The status bar shows *Generating 2 of 5…*
    while any operation is pending. There is no Generate button, because
    there is nothing to press: an edit is its own request. *Stop* cancels
    generation on a huge project; after a Stop the status bar shows
    *Stopped* and **Regenerate all**, which forces every operation to
    generate again and is the only place that command appears (not the
    View menu, not the Warnings drawer). The next project edit also ends
    the stopped state, since it regenerates what it touches.

### Preview

72. **Viewport** (`01-workspace.svg`, centre; `09-preview-legend.svg`).
    The stock from above at the project's scale, Y up, in project units:
    the stock outline, a 10 mm grid that thins and coarsens with zoom, the
    origin crosshair, the park point, the shapes (closed solid, open
    dashed, selected highlighted, hidden absent, locked with a padlock at
    their corner), the pictures, the placed tabs, and the toolpaths. When
    the project has an Inlay, the **male board** is drawn to the right of
    the stock with a gap, as `--preview` draws it (#498): its own outline,
    origin mark, the targets mirrored, and the male runs' toolpaths; its
    shapes are not selectable, since they are the female's instances seen
    from the other board, and clicking one selects the Inlay.
    Toolpaths draw as `--preview` draws them (capability 31): cutting
    moves solid in the operation's colour, rapids thin grey dashed, start
    points as a bit-sized circle with a tick in the direction of the first
    cut, tab marks heavy, drill holes as bit-sized circles. Direction
    arrows along cuts are a toggle. Pan with the middle button, Space-drag
    or a two-finger scroll on a trackpad (open question 8), zoom with the
    wheel or pinch about the cursor, *Fit* (F) to
    the stock and shapes, and *Zoom to selection* (Shift F). Clicking a
    toolpath selects its operation. The Viewport flips Y exactly once;
    every coordinate the user sees is project coordinates.

    The Viewport's controls float over it (`redesign.md`, "The
    Viewport's floating controls"), in three groups:
    - **Top left**: *Select* and the *Tab tool* (69), the one in use
      shown pressed; then *Fit*, *Zoom out*, the zoom as a percentage,
      and *Zoom in*. The percentage is of 96 pixels to the inch, about
      the work's real size on most displays (the app does not know a
      display's true size, so there is no 1:1 button); a click on it sets
      the zoom to 100 %. *Zoom in* and *Zoom out* step by a quarter about
      the Viewport's centre.
    - **Top right**, once the project has a toolpath: the colour mode,
      *By operation* or *By depth*, and *Layers*, which opens the
      toolpath view (73). The depth legend (73a) hangs under them.
    - **Bottom left**: the read-out, `X 64.2  Y 51.8` from the origin
      while the pointer is over the Viewport, and the Z range while the
      program extents are shown (74). Off the Viewport there is no cursor
      part, and no dashes in its place.

    They fade to 60 % while a drag is under way on the Viewport, and Fit
    keeps the stock clear of them, of the Warnings drawer and of an
    Inspector lying over the Viewport. In a Viewport too narrow for both
    top groups, *Layers* is its icon alone, and in a narrower one still
    the zoom controls go.
73. **Toolpath view** (`09-preview-legend.svg`; reworked by the review,
    2026-11-10, and by `redesign.md`, slice D4). What the Viewport draws
    of the toolpaths, set from the Viewport's top-right controls (72),
    which are hidden until the project has a toolpath, so a project fresh
    from a drawing shows only the drawing. The **colour mode** is the
    control itself: **By operation** (default; each operation its own
    colour, repeating after eight, the same palette as `--preview`; the
    colour is the operation's place in the Operations list, so it is the
    colour of its row's glyph, of every tool run of its toolpath, a
    clearing pass lighter (70) and both boards of an Inlay, of its target
    marks (67) and of its swatch in the key, and unticking an operation,
    or one failing, moves no other operation's colour. `--preview` counts
    a colour for each tool run it draws, so its file gives the same
    operation another colour once a project has a clearing pass, a male
    board or an unticked operation) or
    **By depth** (every cutting move coloured from pale at the stock top
    to dark at the deepest cut in the project). **Layers**, beside it,
    opens a popover under it, closed by a click elsewhere or Escape; View
    ▸ Toolpaths… opens it too. The popover holds the **key** for the mode
    chosen (by operation, each operation's swatch and name; by depth, a
    line saying how the ramp reads), then toggles for *Cuts*, *Rapids*,
    *Plunges*, *Direction arrows*, *Start points*, *Tab marks*, *Only the
    selected operation* and *Program extents* (74), and *Show all* /
    *Hide all*. Each operation's eye (65) hides or shows its toolpath.
    These are view settings: not in the file, not undoable, remembered
    between sessions. By operation is the default because it matches
    `--preview` and the quick-start pictures (open question 6, decided
    2026-11-02).
73a. **Depth legend.** While By depth is on, a legend hangs under the
    Viewport's top-right controls, whether or not the popover is open,
    and Fit stands clear of it, beside it or under it, whichever draws
    the stock larger. It
    is one vertical scale of Z in project units, from the clearance
    height at the top to the deepest cut at the bottom, labelled in work
    coordinates (so with Z zero at the stock bottom the stock top reads
    `+11.8`): above the stock top, the band where the bit moves without
    cutting, in the rapid grey, with ticks at the **clearance** height and
    each distinct **safe Z** (`+10 clearance`, `+5 safe Z`); the stock top
    (`0 stock top`); then the cutting ramp with a tick at each
    operation's depth, named (`−6 Square recess`). Rapids and retracts
    keep their grey dashes on the Viewport in this mode; the legend's
    grey band is their key.
74. **Program extents.** A toggle draws the program's XY extents box in
    machine coordinates as `check` prints them (capability 4): the tool
    centre's box and, dashed outside it, the box widened by the largest
    tool's radius, where the cutter's edge reaches. The Viewport's
    read-out (72) shows the Z range. This is the box the user checks
    against their clamps.
75. **Selection on the Viewport.** Click selects an instance (the topmost
    where several overlap; a second click cycles), Shift-click adds,
    Ctrl-click toggles, a drag on empty stock draws a marquee, Escape
    clears. Hidden instances cannot be selected; locked ones can be
    selected but not transformed. Hovering an instance shows its name in
    the status bar. The selection is one selection, shared by the Design
    Tree, the Viewport and the Inspector's Properties.

### Checking and exporting

76. **Warnings drawer** (`10-warnings-panel.svg`). Always current: it is
    `check` on the project as it stands, run with every generation. Three
    tiers: **errors** (export refuses), **warnings**
    and **hints**. `check` prints an error as an `error:` line and both a
    warning and a hint as a `warning:` line with its code; the drawer
    tells a hint by its code. Each row: the tier, the code (monospace, the chapter's
    anchor), the message with the operation and shape named, and, where the
    code concerns a field, that field. Clicking a row selects the operation
    and scrolls to the field, or selects the shape; the row's code links to
    its entry in the Warnings chapter. Rows group by operation, project-level
    rows (thickness unset, tool-change height on a post without `G53`,
    `tool_change_avoidable`) first, under a heading naming the group
    (*Project*, *Panel · Pocket*). When every row belongs to one group the
    heading is not shown, since it would label the only thing there; the
    row still names its operation in its message. The status bar shows the
    counts; the drawer opens from them or from View ▸ Warnings. Its header
    holds the counts, *Copy* (the drawer's text on the clipboard in
    `check`'s format) and close; it has no Regenerate button (71).
76a. **Carve notes** (`10-warnings-panel.svg`; review 2026-11-10). The
    summary `check` prints for an operation that carves (deepest cut, flat
    floor, floor ridges, start plane, the male board's band and cut-out)
    is information, not a problem, so it is neither in the editor nor
    counted with the warnings. It is shown as a **note** in the drawer,
    last in its operation's group, with a quiet `info` glyph in secondary
    text (not the hint blue), laid out as labelled facts, one per line,
    under a sub-heading per board when there are two:

    | Female board | |
    | --- | --- |
    | Deepest cut | 4.000 mm below the stock top |
    | Flat floor | at 4.000 mm (depth_mm) |
    | Floor ridges | 0.400 mm along the rings, up to 0.800 mm at corners no clearing pass reaches |

    The status bar's message cell shows the selected operation's first
    fact (*Lettering · deepest cut 1.658 mm, no flat depth*; for a Photo
    V-Carve its grooves, *Portrait · 42 grooves 2.309 mm wide, to
    2.000 mm*) and opens the
    drawer at its note. *Copy* writes the notes as `check` prints them,
    so the text the CLI prints is unchanged.
77. **Export** (`11-export-dialog.svg`; trimmed by the review,
    2026-11-10; made a step by `redesign.md`, slice D4). Export is the
    Export step, not a sheet: File ▸ Export G-code… and Ctrl E open the
    step, and the header's **Export G-code** (50) is the command. Under
    the readiness card (50) the step shows, top to bottom:
    - **What blocks export**, while an error does: a card in the error
      colour that counts them (*1 error blocks export*), with *Show*
      opening the drawer at them, and under it each finding, its message
      and the field it is about. A click on one opens the step that fixes
      it, as its row in the drawer does (76). *Export G-code* is disabled
      with the same reason in its tooltip. Warnings are not listed here:
      the status bar counts them.
    - **The output settings** (60, 62): the Post-processor, Heights and
      Tolerances groups, each closed until it is wanted, with one line of
      its values on its header (*grblHAL v4 (latest) · .nc*); a group
      holding a finding shows its count, and a finding's row opens it.
      Switching the post-processor is the step's switch (62), an undoable
      project change the user saves or not.
    - **Programs**, where the enabled operations cut with two tools or
      more: the program split, *as the project says* (the setting's value
      shown) or overridden for this export to one program per tool or one
      program with tool changes (`--programs-per-tool`, `--one-program`;
      the override is not saved, and *one program* is refused on a post
      without `M6` with the CLI's message). With an Inlay, a tick per
      board, **Female board** and **Male board**: unticking one exports
      the other alone for this export (`--board`), not saved, which is
      how the male is re-cut after a fit adjustment.
    - **Program Details**, a disclosure **closed until the user opens
      it** (and then left as they left it), which lists the programs it
      will write, one row per tool run (capability 3): file name, tool,
      operations, estimated time (capability 30) and Z range (capability
      4), with the step before each (zero the tool, change to T2) as
      `export` prints them, grouped by board with an Inlay. Closed, it
      says how many there are. While toolpaths generate or the programs
      are built, it says which it waits for.
    - **The destination**: the folder, with *Choose…* opening the native
      folder dialog (default the project's own folder, remembered per
      project in the app's preferences), and the file name, with the
      names it will write under it (*Writes plaque-1-T2.nc,
      plaque-2-T1.nc*).

    The step does not repeat the estimated total (its readiness card
    shows it), does not offer the toolpath preview SVG (the Viewport is
    the preview; `export --preview` still writes it from the CLI), and
    carries no note about how files are written. *Export G-code* writes
    every file or none, as the CLI does (#71), never over the project
    file, then shows the result (77a); a project never saved has no
    folder, so it asks for one first with the folder dialog, and writes
    nothing when the dialog is cancelled. When nothing could be written
    the step says why (*Nothing was written: …*) under the readiness
    card. Export never changes the project file.
77a. **Export result** (`13-export-result.svg`; review 2026-11-10). The
    result takes the plan's place on the step (the programs and the
    destination), in three parts:
    - **Status**: a success glyph and *2 programs written*, with **Done**
      beside it, which brings the plan back, to export again or somewhere
      else.
    - **Files**: one row per program in run order, number, file name in
      the monospace face, tool, estimated time; then the **folder** they
      were written to, with *Copy path*, *Show in folder* and **Save
      setup sheet…** (77b).
    - **Run instructions**, a disclosure closed by default: per program,
      what to do before it runs (*Fit T2 1/8 end mill and set work zero X,
      Y and Z with it*), then the controller notes `export` prints (the
      `G43.1` and `G51` lines), each a separate callout, then the extents
      and Z range.

    The result stays until *Done*, or until the project changes, since it
    is then no longer what the project would write. The CLI's report text
    is not shown as one block; every fact in it is in one of the three
    parts.
77b. **Setup sheet** (`13-export-result.svg`; maintainer decision
    2026-11-10). *Save setup sheet…* writes one self-contained HTML file,
    default `<stem>-setup.html` beside the programs, that the user opens
    and prints from a browser; the app does no printing of its own. It
    holds: the project name, the export date, the post-processor and its
    version, the stock (width, height, thickness), the origin (XY zero and
    Z zero), the display units; then each program in run order with its
    file name, its tool (name, number, type, diameter, angle, vendor and
    product), its operations, the step before it, its estimated time, its
    extents (tool centre and bit edge) and Z range; then the export's
    warnings and the run instructions' controller notes. It has no script,
    no external resource and no image, prints on A4 and Letter in black
    on white, and uses the project's display units. Its text is the CLI's
    text where the CLI has one (step-before lines, notes, warnings), and
    the rest comes from the string table (80). Writing it is not an
    export: it never writes a program and never changes the project.
78. **Estimate and extents** appear outside the Export step too: each
    operation's row shows its estimated time on hover, and the status bar
    shows the program's total. The numbers are the CLI's (`[estimate]` in
    `config.toml` is read for the rapid rate, as today).

### First run and help

79. **Start screen** (`redesign.md`, "The start screen"; reworked by
    the review, 2026-11-10). While the project has no shape, picture or
    operation and the Design step is open, the Viewport's card shows the
    start screen in place of the empty stock: the heading *Start with a
    drawing* and one sentence, *Bring in an SVG or DXF drawing to make
    toolpaths from, or open an example to look round.* Under them:
    - Three ways in, in a row: the primary **New from drawing…** (SVG or
      DXF; it is *New Project from Drawing*, so the stock is sized round
      the drawing), the secondary **New from picture…**, and a quiet
      *Open…*.
    - **Examples**: the three quick-start projects (coaster, plaque,
      sign), three cards in a row, each with a **thumbnail** of its
      shapes drawn from the example itself (not a stored image), its name
      and one line of what it teaches (*Profile with tabs*, *Pockets and
      holes*, *V-carved lettering*). Opening one opens an untitled copy,
      so Save asks where; the card says nothing about it.
    - **Recent**: the most recent files that still exist, each opening
      on a click.

    A drawing, a picture or a `.dcv` dropped anywhere on the window does
    what the matching button does (51, 55, 56). The screen goes when a
    shape arrives, and another step shows the stock in its place. It has
    no copy about phases or drawing tools. The step panel shows the
    Design step with *No shapes yet*; an empty Tools list says *No tools
    yet* with *New tool* and *Add from library…* as buttons; an empty
    Operations list says *Select shapes on the drawing, then Add an
    operation*. Help ▸, and the rail's Help, open the book's quick start
    for the app (*Quick Start*), the Keyboard chapter (*Keyboard
    Shortcuts*) and its front page (*User Guide*) in the browser, and
    *About DreamCarve*, which says the version in the status bar. The
    pages are the published book's, so until this phase's chapters are
    published the first two are not there yet.
79a. **Menus** (`14-menus-and-sheets.svg`; review 2026-11-10). **File**:
    New; New Project from Drawing…; New Project from Picture…; Open…;
    Open Recent ▸ | Save; Save As… | Add Drawing to Project…; Add Picture
    to Project… | Export G-code… (which opens the Export step, 77); Save
    Setup Sheet… (after an export in this session) | Quit. **Edit**: Undo *name*, Redo *name* | Duplicate,
    Delete, Rename | Select All. **Arrange** (58a).
    **View**: Step Panel | Go to Design, Go to Stock, Go to Tools, Go to
    Operations, Go to Export | Toolpaths…, Warnings | Fit View, Zoom to
    Selection, and **Appearance ▸** with three checked choices, *System*
    (following the OS), *Light* and *Dark* (the default), kept in
    `gui.toml`. **Project**: Project Settings…, Tool Library…, Fit Stock
    to Shapes… (a submenu that takes the margin). **Help**: Quick
    Start, Keyboard Shortcuts, User Guide, About DreamCarve (79). A choice of three is a submenu with a check,
    not a checkbox, since a checkbox cannot say "follow the system".
80. **Strings.** Every user-visible string in the app comes from one
    string table keyed by id, so phase 8 can translate it; English is the
    only language shipped. Warning text is the CLI's text, from the same
    source.

## Out of scope

- Drawing or editing shape geometry: lines, rectangles, circles, text,
  node editing, fillets, booleans, offsets, joining open paths, snapping,
  alignment, distribution, arrays, folders, construction geometry (phase 5).
- Rotating or mirroring a picture, or stretching it out of its pixels'
  proportions (phase 3 decision; revisit in 5). Its size can be retyped
  (56).
- 3D preview and material-removal simulation (phase 6).
- Machine-definition editing (ADR-0004: definitions are bundled); a
  post-processor converter (phase 8). Editing library tools in place is
  **in** this phase since the review (64a, maintainer 2026-11-10).
- Operation templates, merging, array copies, tiling, nesting, the
  operation booklet, CAM Plan, clamps as obstacles (phase 8 or post-1.0).
- Printing from the app: the setup sheet (77b) is an HTML file the
  browser prints. Printing the design or exporting the design as SVG
  (phase 8).
- Writing the toolpath preview SVG from the app (`export --preview`
  writes it; the Viewport is the app's preview).
- The assistant panel (its own proposal; open question 3).
- Autosave and crash recovery files (maintainer 2026-11-10: they need a
  crash-recovery design and get their own issue), more than one project
  open at once, multiple windows, tabs, touch and tablet layouts, a web
  build.
- Localisation beyond keyed strings; appearances beyond System, Light
  and Dark.
- New operation or engine settings: the review moved fields, it added
  none (maintainer 2026-11-10).
- Any change to toolpath generation, warnings or G-code: the engine is as
  phase 3 left it. A GUI slice that needs an engine change opens an issue
  against the engine first, as the Inlay editor did (#498, phase 3.3).

## Acceptance tests

Numbered from 62, each traceable to a capability. The app is driven by its
test harness (the design spec names it) with the same fixtures the CLI
tests use; "the file" is the `.dcv` the app saves. "Equals the CLI" means
byte-identical output with the program's header date pinned (`--date`).

62. **Open, save, unchanged** (51). Each fixture under `tests/golden/`
    and the three quick-start projects opened and saved to a new path is
    byte-identical to the original after the CLI has migrated it to the
    current version. The history is empty after open.
63. **New Project from Drawing** (51). *New Project from Drawing* on `fixtures/coaster.svg`
    yields the project `dreamcarve init` writes for it with the placeholder
    tool and the Profile removed: same stock, same instances, same
    post-processor, no tools, no operations (open question 1).
64. **Import DXF** (55). Importing `fixtures/plaque.dxf` into an empty
    project adds the shapes `init` would, named by layer, selected, as one
    undo step; a DXF with no `$INSUNITS` disables the *Add drawing to
    project* sheet's *Add shapes* button until units are chosen, with the
    importer's message (the CLI's, less its `--units` hint, which names a
    flag the sheet does not have); ticking one layer imports that layer
    only; the unread-entity warning appears once in the Warnings drawer.
65. **Add a picture** (56). Importing `fixtures/photo/gradient.png` at
    width 100 adds the bitmap `image add --width 100` adds, at the origin,
    with the grayscale note for a colour file.
66. **Move, rotate, scale, mirror** (58). With the coaster selected,
    typing X 10 and Y 5 changes the instance's transform by that
    translation and nothing else; the toolpath regenerates translated by
    the same vector (compared to the CLI's toolpath for the saved file);
    rotation 90° and scale 50 % are the instance's transform followed by
    that rotation or scale about the selection's bounding-box centre
    (rotation through the Arrange group's Rotate popover, 58a); Flip
    horizontal and vertical flip about the bounding-box centre. A locked
    instance refuses a drag and a nudge with a status-bar message; a
    typed value, Rotate and Flip cannot be asked of it at all, since its
    fields are read-only and the Arrange group is disabled with the
    reason in its tooltip (58, 58a). Each change is one undo step and
    undo restores the transform exactly.
67. **Linked copy** (59). Duplicate yields a second instance with the same
    definition id; the file holds one definition and two instances; moving
    one leaves the other.
68. **Delete a target** (59). Deleting the plaque's `outline` removes it
    from the Cut out Profile's targets, leaves the Profile with
    validation's `operation_has_no_targets` error in the Warnings drawer
    (the row is the error's message, as `check` prints an error, without
    its code), and removes the outline's placed tabs; undo restores all
    three. Deleting `square`, the one target of the Square recess Pocket,
    leaves that Pocket with the same error.
69. **Stock, origin, heights** (60, 61). Setting the stock, origin presets
    and custom origin, Z zero, and the three heights writes the fields
    `check` reads and the Viewport's crosshair and park mark move; the
    tool-change height is disabled on the generic post with the CLI's
    message; *Fit stock to shapes* reproduces `init`'s stock for the
    coaster; the thickness field is marked and export is disabled until it
    is set.
70. **Post-processor switch** (62). Changing grblHAL to LinuxCNC on a
    project with a customised dwell keeps the dwell and resets what
    `switch_post` resets (acceptance 39); *one program* is offered on
    grblHAL and absent on GRBL; canned cycles is offered where the
    post-processor's definition has them, which of the six bundled ones is
    LinuxCNC and Mach3 (`G81` and `G82` in both), and on none of generic,
    GRBL, grblHAL and UCCNC.
71. **Tools** (63, 64). Adding the 6.35 mm end mill from the library writes
    the copy `tools add` writes; clearing a tool's plunge rate puts the
    red dot on its row in the Tools step, lists the CLI's refusal in the
    Warnings drawer and disables the header's *Export G-code*; typing it
    back clears all three. *Delete* on a tool in use is disabled, its
    tooltip naming the operations that use it, and the command behind it
    is refused in the same words. The Tool Library sheet's import of
    `fixtures/fusion/sample.tools` is a review in place (64b): its counts
    and each tool's status are those of the report `tools import` prints
    (acceptance 35), and the library file equals the CLI's.
72. **Add an operation** (65). With the coaster selected, *Add ▸ Profile*
    appends a Profile targeting it with `init`'s Profile defaults, the
    project's end mill, and the name `Profile 1`; with nothing selected the
    menu says to select shapes; with an open path selected, Pocket is below
    the divider with its reason.
73. **Every field round-trips** (66, 68). For each kind and each field in
    the table in 68, the harness sets the field to a non-default value,
    saves, and the CLI reads back that value; sets it to its default and
    the file holds the default (or `null` where the file uses it); and the
    editor shows no control the table does not list and lists no control
    the editor lacks. Tool-derived placeholders show the tool's value and
    write `null`.
74. **Reorder, enable, duplicate** (65). Dragging the plaque's Bore below
    its Panel pocket reorders the file's list and the order of the
    written programs; unticking an operation sets `enabled: false` in the
    saved file and removes its toolpath from the Viewport and its cuts
    from the written programs; *Duplicate* puts a copy below its original
    (65), with the same targets and `(2)` in its name.
75. **Clearing passes** (70). Adding two passes with a 6.35 mm and a
    3.175 mm end mill writes the `clearing` list in that order; dragging
    the smaller above the larger shows `clearing_not_descending` under the
    list, as `check` reports it.
76. **Inlay, both boards** (68, 68a, 72, 77). The lettered inlay fixture
    (one file, #498) shows the Inlay's values once and a Male board group;
    with the male thickness cleared, `inlay_male_board_unset` sits under
    the field, the male board on the Viewport is empty, and the Export
    step lists the female board's programs only. With it set again the
    male board shows the design mirrored with the band, floor rings,
    cut-out and tabs as `--preview` draws them (compared mark for mark at
    the fixture's own 12 mm; at 18 mm, that the male is cut and drawn),
    `inlay_male_mirrored` lists the same X the CLI prints, the derived
    line shows the start plane, floor and engagement the design spec
    computes (red when the engagement exceeds the cone height), and the
    Export step lists both boards' programs with the CLI's names.
    Unticking the female board exports the male programs alone and leaves
    the file unchanged. Exporting both gives the programs `dreamcarve
    export` writes for the fixture, which is the project the inlay quick
    start builds (no program of the pair is committed). With the fixture's frame
    chosen in the Boundary row and the margin and rounding cleared, the
    project is the one the Inlay chapter's hand edit makes
    (`tests/fixtures/dc-boundary.dcv`), the male board shows the frame
    dashed with the male's cuts ending at it, and both boards' programs
    equal the CLI's for that project.
77. **Photo V-Carve** (56, 68). The gradient fixture with a Photo V-Carve
    at `init`'s defaults, its maximum depth typed (`init` leaves it unset
    and the app never fills it, S20), draws the grooves `--preview` draws;
    `check`'s groove count is in the status bar's message cell while the
    operation is selected, and in its note in the Warnings drawer (76a).
78. **Tabs** (69). *Place tabs* on the coaster's Profile with a request of
    four writes the four placed tabs `tabs place --count 4` writes and
    clears the request; dragging one along the outline changes only its
    point and keeps it on the outline; deleting one leaves three; the Tab
    tool adds one at the clicked outline point; the exported program equals
    the CLI's for the saved file.
79. **Preview equals the toolpath** (72). For each golden case that
    holds a DreamCarve project (a `dreamcarve.dcv`; the others hold
    PureCutCNC's outputs alone), the set
    of moves the Viewport draws for an operation is the generator's
    toolpath for the saved file (same count, same endpoints to 1 µm), and
    the Viewport's SVG dump equals `--preview`'s file for that project.
80. **Depth colouring** (73, 73a). On the plaque, *By depth* colours the
    Pocket's floor moves by their Z and the legend's labels are the
    clearance height and safe Z above the stock top (positive, in the
    rapid grey band), the stock top, each operation's depth, and the
    deepest cut, in project units and work coordinates; with Z zero at
    the stock bottom the stock top's label is the thickness.
81. **View state is not project state** (73). Toggling every visibility
    control, panning, zooming and resizing panels leaves the file
    unchanged on save and adds nothing to the undo history.
82. **Warnings drawer equals `check`** (76). For each fixture under
    `crates/dreamcarve/tests/fixtures/` with expected `check` output, the
    drawer lists the same codes, messages and operations, an error as an
    error and a warning as a warning; a hint, which `check` prints as a
    `warning:` line, is the drawer's third tier, by its code. Clicking a
    located row selects the operation and focuses the field;
    *Copy* yields `check`'s text.
83. **Export equals the CLI** (77, S20). The coaster, the plaque, the
    sign and the inlay pair's project (one file since #498) exported from the app, with the date pinned, give
    files byte-identical to `dreamcarve export` on the saved file, for each
    of the six post-processors, with and without the program-split
    override; the Viewport's SVG dump equals `--preview`'s (79).
84. **Export is blocked by errors only** (77). A project with a tool
    lacking a feed shows the error on the Export step, where a click on
    it opens the tool, and a disabled *Export G-code*; a project with
    only warnings exports, and the status bar counts them.
85. **Override not saved** (77). Exporting with *one program* overridden
    leaves `tool_change_mode` unchanged in the file.
86. **Undo and redo** (52). Twenty mixed edits (move, field, reorder, add,
    delete, import, place tabs) undo in reverse order to the opening
    state and redo to the final state, each with the labelled name; a new
    edit after undo clears redo; typing three characters in a field then
    leaving is one step.
87. **The window never blocks** (71). On a project of 500 shapes with a
    Pocket and a V-Carve on all of them, the window answers input within
    100 ms while generation runs (measured over the frames drawn
    meanwhile, as the design spec's test plan reads it after #573: the
    95th percentile within 100 ms and no frame over 250 ms), the rows
    show spinners, the stale toolpaths show faded, and *Stop* ends
    generation within a second.
88. **Unsaved changes** (51). Closing a changed project shows the sheet
    with *Save*, *Don't Save* and *Cancel* and no other button; *Cancel*
    keeps it open and unchanged; *Don't Save* loses the changes; *Save*
    writes the file before closing. Quitting, *Open* and each *New* do the
    same.
89. **Units** (53). An inch project shows every length in inches with
    `in`; typing `1/4 in` in a mm project's depth stores 6.35; typing `0.5`
    in an inch project stores 12.7; the file is unchanged by switching the
    display and back.
90. **Quick starts without the CLI** (exit). The coaster, the plaque, the
    sign and the inlay pair built in the app from their SVG or DXF by the
    steps of the new quick start export the programs the command-line
    quick starts make. The plaque's, the sign's and the pair's are
    byte-identical to what `dreamcarve export` writes for the committed
    project each of those quick starts builds, with the tools added in
    the chapter's order. The coaster's is the committed
    `coaster-grblhal.nc` but for two comment lines, the operation's name
    and the tool's: the CLI's coaster keeps `init`'s placeholders
    (`Profile`, `Set me`) and the app has no placeholder; renamed to
    them, it is that program byte for byte.
91. **Keyed strings** (80). A scan of the window's sources finds no
    string literal a user could read outside the string table; every key
    in the table is used. Error and refusal text is the CLI's and
    `dc-app`'s `Display`, as 80 says of warnings, and is left to phase 8.
92. **Three platforms** (50). The app launches, opens the plaque, renders
    one frame and exports it in CI on Windows, macOS and Linux (the design
    spec names the headless harness).

Added by the review redesign (2026-11-10); the design spec's review
slices carry them.

93. **Window title, steps and Inspector** (50, 50a). The plaque opens
    as `plaque — DreamCarve`; an edit makes it `plaque* — DreamCarve`; a
    new project is `Untitled — DreamCarve`; the header shows the same
    name, and no widget shows `plaque.dcv`. The rail's steps are Design,
    Stock, Tools, Operations, Export; a new project opens on Design, the
    Coaster on Stock with its error marked, and the plaque on Export
    with the four steps before it complete; the step picked stays through
    an edit. *Continue* opens the next step that is not complete; on
    Export the primary button is *Export G-code*, which writes the
    programs and is disabled once a tool an operation uses has no feed. With nothing selected there is no
    Inspector; with a shape selected on Design it shows the Properties
    and the Arrange group, and its close button deselects. At 1280 × 768
    the four regions sit side by side at no less than their minimum
    widths; in a narrower window the Inspector lies over the Viewport.
94. **Status bar and floating controls** (50b, 71, 72, 73). The status
    bar never shows the stock size, the cursor or a Toolpaths button; the
    Viewport's read-out has no cursor part off the Viewport; the colour
    mode and *Layers* are absent on a project with shapes and no
    operation and float at the Viewport's top right once a toolpath
    exists; the generation cell is absent when idle, and the estimate is
    in its place; after Stop it shows *Regenerate all*, which no menu and
    no drawer offers; the units menu switches the display units as one
    undo step. The units and the counts stay where they are when the
    generation cell or the pointer's cell comes or goes. A blank project
    shows *No problems* and no count on the rail's Warnings button. At
    the minimum window with an operation in the Inspector, the stock as
    Fit frames it is clear of every floating control, with By depth on
    or off.
95. **Arrange** (58, 58a). With the square selected, the Inspector's Rotate
    turns it 90° clockwise, the popover's angle 30 rotates it 30°
    anticlockwise, and each flip mirrors it about its bounding-box centre,
    each one undo step equal to the transforms of 66; the Properties
    offer X, Y, W and H only; with the proportion link broken,
    W changes alone. With only a locked shape selected the group is
    disabled with the reason in its tooltip.
96. **Design Tree** (57). The squiggle's row carries the open-path glyph
    and no `open` text; F2 and *Rename* each rename a shape in place as one
    undo step; an empty Pictures section has no *None* row.
97. **Editor layout** (66, 66a, 66b). For each kind, the editor's first
    rows after the header are Targets, Tool and that kind's Setup rows of
    66a in that order; there is no Identity group, no name field below the
    header and no enabled checkbox in the editor; renaming in the header
    writes the name as one undo step; every other group is closed on first
    show with a summary of its values; a group holding a located warning
    opens; every field of 68 is shown exactly once (acceptance 73's table
    is unchanged). A tool with no feed shows *Required* and the glyph in
    the Feed field and no message line under it, and its tooltip is the
    file's message.
98. **Targets** (67). Selecting the plaque's Pocket draws its two targets
    in its colour and fades the rest, and marks their tree rows; target
    editing adds the square on a click and removes the island on a second
    click, *Done* writes the new list as one undo step, *Cancel* leaves the
    file unchanged; *Add to "Panel"* in the tree's context menu adds a
    shape.
99. **Carve notes** (76, 76a). On the sign, the Lettering V-Carve's
    summary appears as a note in its drawer group with *Deepest cut* as a
    labelled fact, is not counted in the status bar, and *Copy* yields
    `check`'s text byte for byte; the inlay fixture's notes have a Female
    board and a Male board sub-heading. With a single group in the drawer,
    no group heading is shown.
100. **New tool** (63). *New tool* on an empty project's Tools step adds an
    end mill named `tool1`, 6.35 mm, 2 flutes, with no feed, plunge rate,
    RPM, step-down or stepover, selected, with its name field focused; a
    second is `tool2`; export is refused naming the unset rates, as the
    CLI refuses it.
101. **Tool Library** (64, 64a, 64b). The library sheet groups the
    fixture library by vendor with *No vendor* last; editing a library
    tool's feed writes `tools.json` at once, the file `dc_library::save`
    gives for the library with that one value changed (the CLI has no
    command that edits a library tool, so there is no CLI file to compare
    with), and leaves the open project and the undo
    history unchanged; *Revert changes* restores it; switching the
    library's units to inches shows `0.25 in` for the 6.35 mm end mill and
    leaves the file and the project's units unchanged; importing
    `fixtures/fusion/sample.tools` twice, the second time after editing one
    of its tools in the library, shows that tool as *Changed*, and *Replace
    with imported* gives the library `tools import --replace` gives.
102. **Export step and result** (77, 77a, 77b). The Export step has no
    preview tick, no warning list, no estimated total and no note about
    how files are written, and its Program Details are closed until they
    are opened; with an error it shows the one blocking line over the
    finding and a disabled *Export G-code*. A project never saved asks
    for a folder first, and writes nothing when the dialog is cancelled.
    After an export the result, in the plan's place, shows the status,
    the files in run order and the folder; *Run instructions* is closed
    and holds the CLI's step-before lines and notes; *Done* brings the
    plan back. *Save setup
    sheet…* writes an HTML file with no `<script>`, no `http` reference and
    no `<img>`, whose programs, tools, step-before lines, extents and
    estimates equal the export's, deterministic with the date pinned.
103. **Start screen and menus** (51, 79, 79a). The start screen shows
    the three actions of 79, three example cards with thumbnails and the
    recent files that still exist, and no sentence about phases or
    unsaved copies; an example opens untitled. The import sheet's preview has no stock outline. The
    File menu's labels are those of 79a; View ▸ Appearance has System,
    Light and Dark with one checked; the unsaved-changes sheet's buttons
    are Save, Don't Save and Cancel.
104. **Visual review of the redesigned window** (50a). The checklist of
    `experience.md` section 13 (one item per rule of `redesign.md` and
    of `experience.md`'s other sections, 100 in all), each item ticked
    against a picture the app wrote, or the test that holds it where a
    picture cannot, or marked *not met* with what is wrong. The pictures
    are of every step, the Inspector on each kind of selection, the tool
    editor, every menu, popover and sheet, the Viewport's floating
    controls, the Export step ready, blocked and done, the window at its
    minimum and narrower, the focus ring, and the operation editor on
    each kind, in both appearances at 1× (section 13 asks for 1×; a 2×
    set is not part of this review). The record is
    `docs/specs/phase-4/review/final/README.md`, which says what each
    group of scenes holds and names the picture or the test beside each
    item; it stands in for the per-picture captions and the checklist
    comment on #549 that the review of the old layout had (#549 and pull
    request #666 are that earlier review, superseded by the redesign).
    Done for the redesign on 2026-11-19: 94 items met and 6 not (4, 15,
    34, 64, 65, 99), four of them places where the two specs ask for
    different things. **Open:** the maintainer's decision on those six
    and sign-off, recorded in that README.

## Safety requirements

S1–S19 apply unchanged: the engine, the safety pass and the post-processors
are phase 3's. New in this phase:

- **S20 The window is a shell, not a path.** The program the app writes is
  `dc-gcode`'s output for the project snapshot the app holds, built by the
  same generation call and checked by the same safety pass as
  `dreamcarve export`, and equals the CLI's program for the saved file
  (acceptance 83). No value reaches the engine that is not a field of the
  project; the app has no settings of its own that change motion. Export is
  refused on the same conditions as the CLI (a tool without feed, plunge or
  RPM; thickness unset; an error in `check`) and never offers to export a
  subset the file does not describe. The app never fills a feed, plunge
  rate, RPM or depth with a number of its own: such fields are empty until
  the user or a library tool supplies them (principle 5). What the Viewport
  draws is the generator's toolpath for the current snapshot; while an
  operation regenerates its old drawing is marked stale and the Export
  step waits for generation to finish. Checked by acceptance tests 71,
  79, 83 and 84 and by a source rule in the design spec: the GUI crate
  depends on `dc-cam` and `dc-gcode` only through the same entry points the
  CLI uses.

## Test part

No new machine output, so no new part. **Bench check:** the coaster
(phase 1 test part) built in the app from `coaster.svg` by the quick start,
exported, and diffed against the phase 1 program; cut once from the app's
program on the bench grblHAL to close the loop. Record below.

| Item | Result |
| --- | --- |
| App program equals the CLI program (diff) | |
| Cut from the app's program, released cleanly | |
| Diameter, two measurements | |
| Date, version, controller, sender | |

## Documentation deliverables

- Quick start: **From SVG to G-code in the app**: the coaster end to end,
  with screenshots, then the plaque's second tool and the sign's V-Carve as
  short follow-ons that point at the phase 2 and 3 quick starts for the
  machining detail. The existing quick starts gain a note that each step
  has an equivalent in the app, with the panel named.
- User guide, new chapters: **The app** (the workspace tour, panels,
  selection, keyboard), **Importing drawings and pictures** (replaces the
  DXF chapter's CLI framing; the DXF detail stays), **Placing shapes**
  (move, the Arrange group, linked copies, lock), **Stock, origin and
  heights**, **Toolpath preview** (colours, the toolpath view popover, the
  depth legend, extents, what the marks mean), **Checking and exporting**
  (the Warnings drawer and its notes, the Export step, the result and
  the setup sheet), **Keyboard shortcuts**.
- User guide, updated: Overview (no longer "a command line tool": the app
  first, the CLI as the same engine for scripts), every operation chapter
  with a screenshot of its editor and the field names beside the file
  names and the Setup rows first, Tabs (placing and dragging), Tool
  library (the sheet, vendor groups, editing in place, library units, the
  import review, the picker, *New tool*), Post-processors (the Export step), Configuration (app
  preferences and where they live), Warnings (the panel; "located at the
  field" now means the editor), Inlay (both boards from one project: the
  male board, the per-board export tick; the two-file workflow gone).
- `CONTEXT.md`: **Design Tree** (the list of a project's feature
  instances, tabs and pictures; avoid: layer panel, object list; PureCutCNC:
  FeatureTree), **Viewport** (the 2D view of the stock from above; avoid:
  canvas, sketch view; PureCutCNC: SketchCanvas), **Toolpath Visibility**
  (the per-operation and per-move-kind display toggles; not project data;
  PureCutCNC: ToolpathVisibility).
- Contributing: Crate map (the GUI crate), How work flows (the GUI test
  harness and the screenshot rule for docs), Porting from PureCutCNC (what
  of its React UI is behaviour to port and what is not).

## Exit criteria

- [ ] Acceptance tests 62–103 green on Windows, macOS and Linux in CI,
      and the visual review of the redesigned window (104) signed off by
      the maintainer in `review/final/README.md`.
- [ ] S20's equality test (83) covers all six post-processors.
- [ ] The coaster, plaque, sign and inlay pair reproduced in the app from
      their drawings, programs identical to the CLI's (90); the coaster cut
      from the app's program and recorded above.
- [ ] Quick start "From SVG to G-code in the app" and every chapter above
      published, with screenshots from the shipped build.
- [ ] `CONTEXT.md` updated with the three terms.
- [ ] Every user-visible string keyed (91).
- [ ] Binaries for all three platforms from cargo-dist; the app opens a
      `.dcv` by file association on each.
- [ ] `v0.4.0` tagged.

## Review redesign

The maintainer reviewed screenshots of the working app on #549
(2026-11-10, about 80 items keyed by screenshot) and recorded decisions
the same day. The capabilities above are amended in place; this is the
record of what changed and why. It speaks of the window as it was then
(a toolbar, a left column with Project Settings, a CAM panel with two
tabs); `redesign.md` has since replaced that layout (50), and what the
review decided about a panel holds for the step or the Inspector that
now has its content.

- **Decisions.** Library tools are edited in phase 4 (64a), with vendor
  groups and their own display units. Autosave stays out. Appearance
  keeps System, Light and Dark (79a). No engine or operation setting is
  added; each editor's required settings move to its top (66a). The
  export result gets *Save setup sheet…*, an HTML file printed from a
  browser (77b).
- **Answers agreed.** *New tool*, named `tool<n>` (63); *New Project
  from Drawing…* and *Add Drawing to Project…* (51, 55); Rename on F2
  and in the context menu (57); *Save*, *Don't Save*, *Cancel* (51);
  *Regenerate all* only in the status bar after Stop (71); no stock box
  in the import preview (55); the project name without extension in the
  window title and the panel called *Project Settings* (50, 60); no name
  above the CAM panel, which gets a title (50); Project Settings and
  Properties as separate collapsible sections, Project Settings on top
  (50); a chain proportion link right of W and H (58); the toolpath view
  opening from the status bar (73) and hidden until a toolpath exists;
  no warnings group heading when there is one group (76); no stock size
  in the status bar (50b).
- **Designer's calls on the open items.** Targets are shown on the
  drawing and edited in a target-editing mode (67); the editor's name is
  in its header and *enabled* only in the list (66); each editor opens
  with a Setup block and closed groups that summarise their values (66,
  66a); unset required values show a *Required* field, not a paragraph
  (66b); carve summaries become structured notes in the drawer and the
  status bar (76a); the export sheet is trimmed and its result is Status,
  Files and Run instructions (77, 77a); the empty state offers two
  distinct paths, the examples pictured (79); open paths get a glyph,
  not a word (57); Tools comes before Operations (50); rotate and flip
  become Arrange actions on the toolbar (58a); the depth legend shows the
  positive Z of non-cutting moves (73a); the library's import becomes a
  review before writing, which is where *Replace* now lives (64b).

The design spec's "Review redesign slices" carry this work. Its second
set of screenshots was to close #549; the window was then redesigned
(`redesign.md`), and the review that closes this phase is the redesign's
own (acceptance 104).

## Open questions

Questions 1–12 were answered by the maintainer on 2026-11-02: every
recommendation accepted. The decisions are folded into the capabilities
they name, and the record stays here. Question 13 was withdrawn the same
day.

| # | Question | Recommendation |
| --- | --- | --- |
| 1 | *New from drawing* (51): does it add `init`'s placeholder tool (feeds unset) and one Profile of every closed shape, as the CLI does, or only the shapes and the stock? | Shapes and stock only. `init` adds them because a file with no operation gives a CLI user nothing to edit; in the app the empty Operations list and *Add* do that, and a placeholder tool with unset feeds is a trap the library picker avoids. Acceptance 63 and the quick start follow the decision. |
| 2 | Importing into a project that already has shapes (55): place the new shapes as drawn (drawing origin at the stock's bottom-left) or centred on the stock, or ask? | As drawn, then selected, so the user moves them if they want. Deterministic, and matches `init`. |
| 3 | The assistant proposal from October (a chat panel docked right, the command layer, a local language model) is not in this repository's `docs/plan.md`. Does phase 4 reserve a place for it, or does it get its own spec and phase later? | Its own spec later. Phase 4's layout (50) must allow a third side panel without redesign, and 52's named commands are the seam it would plug into; nothing else here depends on it. |
| 4 | Export destination (77): ask every time, or remember the last folder per project in app preferences? | Remember per project in app preferences; the default is beside the project file. Never in the `.dcv`. |
| 5 | Per-export operation subset, as PureCutCNC's export dialog offers? | No. Enabled is the project's state and the file's truth (S20); the dialog shows what will be written. |
| 6 | Default toolpath colour mode (73): by operation or by depth? | By operation; it matches `--preview` and the quick-start pictures. Depth on a toggle. |
| 7 | Editing library tools in place in phase 4, rather than phase 5 as the plan says? | Keep phase 5. *Save to library* from a project tool covers the common case now. **Reversed 2026-11-10** by the screenshot review: library editing is in phase 4 (64a). |
| 8 | Pan gesture (72): middle-drag, Space-drag, or both? | Both, plus two-finger scroll on a trackpad. |
| 9 | Appearance: must both light and dark ship in phase 4, or is following the OS with one palette enough? | Both, since following the OS implies both; the palette is the design spec's and the toolpath colours must read on both. |
| 10 | Phase 3.5 (Photo V-Carve, format v27; it was 3.4 and v25 when this was asked) may land after phase 4 opens. Gate its editor as one slice on v27, or hold phase 4 for it? | Gate the slice. Everything else in this phase is independent of it. |
| 11 | Where app preferences live (layout, recent files, visibility, export folders): beside `config.toml` in the user config directory? | Yes, as `gui.toml` beside it, documented in the Configuration chapter. Never in the `.dcv`. |
| 12 | Window and panel minimums (50): is 1280 × 768 the floor, or must the app work on a 1366 × 768 laptop with panels collapsed? | 1280 × 768 with the rail, the step panel, the Viewport and the Inspector at their minimum widths; in a narrower window the Inspector lies over the Viewport rather than the window refusing. |
| 13 | Inlay sibling check: should the Warnings panel compare a `<stem>-male.dcv` beside the open female? | Withdrawn 2026-11-02: the maintainer asked why there was a side at all, and the answer was the file. #498 puts both boards in one project (68a), so there is nothing to compare. |

## Mock-ups

Mock-ups, generated by `mockups/render.py`; `mockups/README.md` maps each
to its capabilities. They show the window before the redesign
(`redesign.md`): a toolbar, a left column and a CAM panel with tabs, in
the earlier light tokens. They are kept for their labels and field
names; the layout is capability 50's, and the egui build is the
authority on pixels. The plaque from the phase 2 quick start is the project
in most frames.

![01 The workspace](mockups/01-workspace.svg)

![02 Import a drawing](mockups/02-import-dialog.svg)

![03 Project Settings: units, stock, origin, heights, post-processor](mockups/03-stock-origin.svg)

![04 Tool Library](mockups/04-tool-library.svg)

![05 Operation editor: Profile](mockups/05-operation-profile.svg)

![06 Operation editor: V-Carve, and Inlay with both boards from one project](mockups/06-operation-vcarve.svg)

![07 Operation editors: Drill, Bore, Engrave, Photo V-Carve](mockups/07-operation-fields.svg)

![08 Placed tabs on the Viewport](mockups/08-tabs-on-canvas.svg)

![09 Toolpath visibility and depth colouring](mockups/09-preview-legend.svg)

![10 Warnings panel](mockups/10-warnings-panel.svg)

![11 Export dialog](mockups/11-export-dialog.svg)

![12 First run](mockups/12-empty-state.svg)

![13 Export result and setup sheet](mockups/13-export-result.svg)

![14 Menus and the unsaved-changes sheet](mockups/14-menus-and-sheets.svg)

![15 Target highlighting and target editing](mockups/15-targets.svg)

![16 Tools tab: New tool and required values](mockups/16-tools-tab.svg)
