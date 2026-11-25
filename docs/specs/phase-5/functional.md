# Phase 5: GUI, CAD — Functional Spec

**Status:** draft, written 2026-11-22 under the local process of
`docs/phase-5-worklog.md`; the maintainer reviews it with the working
build, as phase 4's redesign was reviewed. Decisions waiting for them are
under "Open questions".
**Design spec:** `design.md`
**Decision record:** ADR-0006 (a drawn shape keeps its outline and its
sizes)

## Goal

A hobbyist can design a part from nothing in the app and cut it: draw
lines, arcs, curves, rectangles, circles, ellipses, regular polygons,
slots, gears and text on the stock; place every point by snapping or by
typing; reshape an outline node by node, round and chamfer its corners,
offset, weld, subtract, trim and join; copy, array, align, distribute,
group and file the shapes; trace over a picture; and measure what was
drawn. Everything phases 1–4 do to an imported drawing they do to a drawn
one, because a drawn shape is saved as the same outline an import makes.

Nothing about toolpaths, warnings or G-code changes. The file gains
format 29: the sizes a shape was drawn with, groups, construction
geometry and a picture's opacity. Every safety rule S1–S20 holds; S21
says a drawn shape is cut by its saved outline and nothing else.

PureCutCNC is the reference for what each tool does (`src/sketch/`,
`src/text/`, `src/cannedShapes/`, `src/components/canvas/`); Graphite is
the second reference for vector-editing behaviour, taken where this spec
says so and why. `docs/research/purecutcnc-drawing-digest.md` and
`docs/research/graphite-tools-digest.md` are the surveys both were read
from. Vocabulary follows `CONTEXT.md`, with the terms this phase adds
listed under "Documentation deliverables".

## Users and scenarios

- **Sam** wants a bench hook plate and has no drawing. *Draw from
  scratch* opens a blank project on the Design step. Sam picks
  **Rectangle**, chooses rounded corners, clicks a corner on the grid,
  types `160` Tab `80` Enter, and has a 160 × 80 mm plate with 8 mm
  corners. **Slot**: two clicks on grid points and a typed width of 6.
  **Circle**: a click that snaps to the plate's corner arc centre, `5`
  for the diameter. *Array* makes the hole four, one at each corner.
  **Text**: a click, the word typed, a bundled font, 12 mm high. Sam
  moves on to Stock, Tools and Operations as in phase 4: the plate is an
  Outside Profile, the slot a Pocket, the holes a Drill, the word a
  V-Carve. No file was imported.
- **Sam, a week later,** needs the slot 8 mm wide. They select it, type 8
  in **Width**, and the slot widens about its centreline; the Pocket
  regenerates. The two linked copies of the slot widen with it. One of
  them should stay 6: *Make Unique*, then 6.
- **Alex** has a logo as a traced SVG with a ragged corner and an open
  outline. They double-click it: its nodes show. They delete three stray
  nodes, drag one into place where it snaps to an endpoint, and the path
  closes. A sharp inside corner gets a 3 mm fillet so the 6 mm bit can
  reach it. They offset the outline 4 mm outward for a border and weld
  two overlapping letters into one shape.
- **Jo** is copying a bracket from a paper drawing. They add a photo of
  it, fade it to 40 %, and scale it by two points: click the two ends of
  the dimensioned edge and type 120. They lock it and draw over it with
  Line, pressing `A` for the arc segments, with construction lines for
  the centre lines. The Measure tool confirms the hole spacing before
  any operation is added. Construction lines are drawn dashed, snap like
  any shape, and are never offered to an operation.

## Capabilities

Numbering continues from phase 4's capability 80 and acceptance test 106.
"The user can" means in the app, on the Design step unless it says
otherwise. Every change to the project is one named undo step (phase 4,
capability 52); what a tool holds before it commits is not project data.

### Tools and the Viewport

81. **Tools.** The Viewport has one tool in use at a time, shown pressed
    in the **tool strip** down its left edge, under the top-left pill
    (the maintainer's review of 2026-11-23; the first build had one
    Draw button with every tool in its popover, which hid them). Rules
    divide the strip into four segments:
    - **Select** (`V`) and **Edit nodes** (`N`, 92);
    - the tools that draw: three **groups**, the **line tools** (Line
      `P`, Arc `A`, Spline `S`), the **shape tools** (Rectangle `M`,
      Circle `C`, Ellipse `E`, Polygon `G`) and the **slot and gear
      tools** (Slot `O`, Gear), then **Text** (`⇧T`);
    - what changes what is drawn: **Trim** (96), **Offset** (94),
      **Join** (97), **Weld**, **Subtract** and **Intersect** (95), and
      **Array** (98). All but Trim act on the selection and are off
      until there is enough of one, two shapes or one for Offset and
      Array, the tooltip saying so; Offset, Join and Array take their
      values in the tool bar, and the three that combine act at once,
      saying why in the status bar when they cannot;
    - what helps with any of it: **Measure** (`K`, 103) and
      **Snapping** (82).

    The **Tab tool** (`T`, phase 4) is not in the strip: it is **Shape ▸
    Tab Tool**, ticked while it is in hand, and its key. A Profile's
    own request places most tabs, so the tool is seldom wanted.

    A group's button shows the tool of the group last used, with a mark
    at its corner that it holds more. A click arms that tool and opens
    the group's list beside the button, each tool with its icon, name
    and key; a row arms that tool, which the button then holds. The
    list is no menu to dismiss: a press on the Viewport while it shows
    is the tool's, and a tool's key still arms it. The top-left pill
    keeps the view's controls, **Fit** and the zoom, and with the tools
    out of it both top pills show whole at the Viewport's least width.

    A drawing tool **stays armed** after each shape (PureCutCNC's sticky
    tools), with the options last used; `Escape` with nothing begun, or
    `V`, returns to Select. Arming a tool opens the Design step. While a
    tool other than Select is armed, clicks belong to it: nothing is
    selected, moved or resized by a click, and the selection handles are
    not drawn. The Draw menu (79a's menu bar gains **Draw**) lists the
    drawing tools with their keys.

    While a tool is armed a **tool bar** floats at the Viewport's top
    centre, under the pills and clear of the tool strip: the tool's name, one line saying what the
    next click does (*Click the first corner*), the tool's options, the
    fields of capability 83, and **Done** where a shape can be finished
    early. A bar with more than fits across the Viewport (the Gear's
    sizes) breaks into rows, a label staying with its field. Edit nodes
    and Trim have the same bar, Edit nodes with its buttons (93). The
    Shape menu's commands that take a value (**Fillet Corners…**,
    **Offset…**, **Join…**) take it in this bar too: the command arms
    as a tool does, its bar shows the value and what the result will
    be, **Done** or `Enter` makes it and returns to Select, `Escape`
    leaves with nothing changed, and where the command cannot act the
    bar says why in place of Done. The value last used is offered next
    time, for the session. The status
    bar's pointer cell shows the same prompt. Fit leaves room for two
    rows of the bar while a drawing tool is armed, so nothing is drawn
    under it, and a fitted view refits as the tool is taken up and put
    down. While a drawing tool is armed the Inspector stays closed: what
    is drawn is selected, and its Properties show when the tool is put
    down. It stays closed too while a command takes its values in the
    tool bar (Offset…, Fillet Corners…, Join…, Array…), whose preview
    is of the selection as it is.

    In a Viewport too narrow for its two top pills, which it never is
    at the window's least size, *Layers* loses its label first, then
    the zoom percentage goes, then the two zoom buttons.
82. **Snapping** (ported from PureCutCNC's `resolveSketchSnap`). Every
    point a tool takes, and every node or shape dragged, snaps when a
    candidate lies within 14 screen pixels of the pointer. The kinds,
    and what each offers:

    | Kind | Candidate | Default |
    | --- | --- | --- |
    | **Endpoint** | every vertex of every visible shape, the stock's corners, the origin | on |
    | **Midpoint** | the middle of every line and arc | on |
    | **Centre** | the centre of every arc and circle, and of every parametric shape (84) | on |
    | **Intersection** | where two outlines, or two parts of one, cross | on |
    | **On outline** | the nearest point of an outline | on |
    | **Perpendicular** | the foot of the perpendicular from the tool's last point to an outline | off |
    | **Grid** | the nearest point of the snap grid (82a) | on |
    | **Horizontal or vertical** | the point level with, or upright from, the tool's last point | on |

    The nearest candidate of the highest rank wins: endpoint, midpoint,
    centre and intersection first, then perpendicular, then horizontal
    or vertical, then on outline, then grid. The grid has no reach: with grid snapping on and nothing
    better near, the point is on the grid (Graphite's rule). With no
    candidate at all the point is the pointer's: a click is never
    discarded (PureCutCNC discards it; that reads as a dead mouse when
    zoomed in). The snapped point shows a marker shaped
    for its kind with the kind's name beside it, and a dashed guide from
    the pointer for *On outline* and from the last point for
    *Perpendicular* and for *Horizontal or vertical*.

    **Horizontal or vertical** (the maintainer's review, 2026-11-23:
    "a way to make a line or object snap to vertical or horizontal")
    puts a pointer within 8 px of the horizontal or the vertical
    through the tool's last point on it; beside that point, where both
    are in reach, the nearer. Its reach is the snap radius where that
    is less, and half a grid step while the grid snaps, so the next
    grid line can still be had. Along the line the point is the
    pointer's, on a grid line while the grid snaps; where the line
    crosses an outline within the snap radius the point is the
    crossing, which ranks with a perpendicular. A node dragged in Edit
    nodes (92) is held the same way to the nodes either side of it that
    are not dragged with it, to both at once where a square corner is
    in reach. `Shift` holds a direction of its own, and this kind
    stands aside for it. It is PureCutCNC's axis lock offered as a snap
    instead of a mode. In `gui.toml` it is `[snap] axis`, apart from
    `kinds`, so preferences saved before it was there have it on.

    Snap targets are the outlines as drawn, so a slot's arc centres, a
    rounded rectangle's corners and a letter's nodes all snap
    (PureCutCNC snaps only to such a shape's frame). Hidden shapes and
    the shape being dragged are not targets; construction geometry (100)
    and locked shapes are. A slot offers its two end centres and its
    middle as **Centre**.

    **View ▸ Snapping** is the master switch (`⇧S` toggles it), a row
    of the View menu so that its key shows beside it, and **View ▸ Snap
    To** has one tick per kind. The tool strip's **Snapping** button is
    pressed while snapping is on, and its list has the same switch and
    the same ticks; holding `Ctrl`/`⌘` while moving the pointer suspends
    snapping for as long as it is held. The settings are app
    preferences (`gui.toml`), not project data. Holding `Shift`
    constrains the point to 15° steps about the tool's last point (the
    step phase 4's rotation handle uses; Graphite's Shift), and a snap
    candidate on that ray still wins. PureCutCNC's `Alt`-tap axis lock
    is not ported as a mode: a held `Shift` is what vector editors do,
    and *Horizontal or vertical* above is the lock as a snap. `Shift`
    held while shapes are moved with the Select tool holds the move
    level or upright, whichever the pointer has gone further along.
82a. **The snap grid.** Its spacing follows the zoom: the smallest of
    1, 2, 5, 10, 20, 50 mm … (0.01, 0.02, 0.05, 0.1 in … in an inch
    project) that is at least 8 px on screen, so a point is never out of
    a grid point's reach and a closer look snaps finer. Every line of the
    grid phase 4 draws (1, 10 and 100 mm; 0.1, 1 and 10 in) is on it, and
    the marker shows where a point between the drawn lines lands.
    **View ▸ Grid ▸ Spacing** pins the spacing to a typed length instead,
    in the project's units, until it is set back to *Automatic*. Held to
    a `Shift` direction, the grid sets the length along it in whole
    steps. The grid's visibility is phase 4's toggle; a hidden grid does
    not snap.
83. **Typed sizes** (PureCutCNC's `manualEntry.ts`, extended). Every
    step of every drawing tool can be typed in the tool bar instead of
    clicked. The bar shows the fields of the step in hand, filled from
    the pointer as it moves:
    - before the first point, **X** and **Y** from the origin
      (PureCutCNC cannot type a first point);
    - Rectangle: **W** and **H**; Circle: **Diameter**; Ellipse: **W**
      and **H**; Polygon: **Radius** and **Angle**; Slot: **Length**,
      **Angle**, then **Width**; Line and Spline: **Length** and
      **Angle** of the next segment; Arc: the end's **Length** and
      **Angle**, then **Radius**; Gear: **Outside diameter**.

    `Tab` moves the focus into the first field and on through them;
    typing a value **pins** it, and the preview then follows the
    pointer only in what is not pinned (type a width and the height
    still follows the mouse). `Enter` commits the step with the fields
    as they read; `Escape` in a field unpins it. Fields take what every
    length field takes (phase 4, capability 53): `1/4 in`, `6.35`,
    `2 1/2 in`, and a formula (`18 / 2 + 0.3`; capability 53,
    "Formulas"). An angle is degrees anticlockwise from +X, as the Y-up
    file is. A value the tool cannot use (a zero width, a radius under
    half the chord) marks the field as phase 4 marks a field's problem
    and commits nothing.
83a. **Measurements while drawing.** The shape being drawn shows its
    sizes beside it in project units: a segment's length, an arc's
    radius, a rectangle's width and height, a circle's diameter, a
    slot's length and width. A selection, of shapes or of pictures,
    shows its bounding box's width and height the same way, under the
    box, while it is dragged or resized (phase 4's status-bar read-out
    stays). While it is rotated it shows none: the status bar reads the
    angle.

### Drawing

84. **Parametric shapes** (ADR-0006; PureCutCNC's canned shapes). A
    Rectangle, Circle, Ellipse, Polygon, Slot, Gear and Text keep the
    sizes they were drawn with. Properties shows them in a **Shape**
    section above the placement fields, in project units, as the shape
    is placed (a shape scaled to twice its size reads twice its drawn
    sizes); typing one changes the shape about its own centre, in one
    undo step named for it (*Set width of “Slot 1”*), and every linked
    copy changes with it (CONTEXT: Linked Copy).

    | Shape | Sizes |
    | --- | --- |
    | Rectangle | Width, Height, Corners (Square, Rounded, Chamfered), Corner size |
    | Circle | Diameter |
    | Ellipse | Width, Height |
    | Polygon | Sides (3–50), Radius, measured to the Corners or to the Flats |
    | Slot | Length (centre to centre), Width |
    | Gear | 86 |
    | Text | 91 |

    A parametric shape resizes as it should. Stretching one by its
    Properties' W or H, or by a corner handle with the proportion link
    open, changes its sizes where the result is still that shape: a
    rectangle's width and height (its corners keep their size), an
    ellipse's, a slot's length and width (stretched across, a slot
    gets wider and its centreline shorter, so its box is the one asked
    for; it is refused when it would be no longer than it is wide); a
    circle stretched becomes an ellipse, and keeps its name. A polygon,
    a gear and a text scale evenly or not at all.
    This holds while the shape sits square to the axes; a rotated one
    scales evenly only. A linked copy cannot be stretched, alone or
    with its other copies: the status bar says *Make Unique to stretch
    this copy*. A shape that cannot take a stretch holds back the whole
    selection, and says why.

    A size the shape cannot have is refused at its field with the
    reason. A corner size is held to half the shorter side, also when a
    side is made shorter. Changing what a polygon's radius is measured
    to leaves the polygon as it is and re-reads the radius. A locked
    shape's sizes are read-only, and so are those of a shape with a
    locked linked copy. A tab on the outline stays at the same share of
    the shape's box, on the new outline.

    A parametric shape is not edited node by node. **Shape ▸ Convert to
    Path** (also in the context menu, and offered when Edit nodes is
    asked of one) makes it a plain path (92) with the same outline, for
    that instance's definition and so for its linked copies; it is one
    undo step and cannot be undone any other way. PureCutCNC's
    equivalent is *Expand to features*.
85. **Rectangle** (`M`). A click at one corner and a click at the
    opposite one; or press, drag and release (Graphite's shape tool;
    PureCutCNC is click-click only). `Shift` makes it square; `Alt`
    draws it from its centre. The tool bar's **Corners** option sets
    Square, Rounded or Chamfered, with the **Corner size** (default
    5 mm or 0.2 in, PureCutCNC's), kept for the next rectangle. A corner
    size is held to half the shorter side. A rounded corner is a true
    arc and reaches the program as G2/G3 untouched.
85a. **Circle** (`C`): a click at the centre and one on the rim, or a
    drag. **Ellipse** (`E`): a click at the centre and one at a corner
    of its bounding box, or a drag; `Shift` makes it a circle. A circle
    is two half-circle arcs; an ellipse is drawn within the project's
    flatten tolerance of the true curve and reaches the program through
    arc fitting, as an imported curve does.
85b. **Polygon** (`G`; PureCutCNC's `ngon`): a click at the centre and
    one that sets the radius and where the first corner points.
    **Sides** (3 to 50, default 6) is in the tool bar and kept between
    shapes. Measured to the Flats, the radius is the distance to the
    middle of a side, the spanner size of a hexagon; PureCutCNC offers
    the corner radius only.
85c. **Slot** (`O`; the note the roadmap carried on slots is folded in
    here).
    A click at the centre of one end, a click at the centre of the
    other, and a third click (or a typed Width) for the width, measured
    across. The slot is two straight sides and two exact half circles
    centred on the two clicked points, saved as arcs. Its Length and
    Width stay editable (84): changing the width keeps the centreline.
    It is an ordinary closed shape to every operation; an inside Profile
    with a tool wider than the slot raises phase 1's `tool_too_large`
    warning, the classic slot mistake.
86. **Gear** (ported from PureCutCNC's `gearProfile.ts`). A click at
    the centre and one for the outside radius; the tool bar then holds
    the gear's sizes, previewed live, and **Done** commits: **Teeth**
    (6 to 200, default 24), **Outside diameter**, **Pressure angle**
    (10° to 35°, default 20°), **Flanks** (Involute, Straight),
    **Whole depth** (default 2.25 modules), **Root** (Rounded, Flat,
    Sharp) and its **Root fillet**, **Crest** (Flat, Rounded) and its
    **Crest radius**, and **Bore diameter** (0 for none). The module is
    shown, derived: outside diameter ÷ (teeth + 2). A bore is a second
    outline of the same shape, a hole to every operation. A size the
    gear cannot have is held as typed and refused at its field with
    PureCutCNC's reason, a limit in it in project units; **Done** is
    off, and `Enter` refused with the reason, while one stands. A size
    shows only while the gear's form reads it: the pressure angle for
    involute flanks, the fillet for a rounded root, the radius for a
    rounded crest. The first radius sets the depth, the fillet and the
    crest radius from the module; after that a new tooth count or
    diameter leaves them as they read. `Backspace` steps back to the
    radius, then to the centre.
87. **Line** (`P`; PureCutCNC's polygon and composite tools as one).
    Each click adds a point joined to the last by a straight segment.
    A click on the first point (within 12 px, three points or more, or
    two when a span between them is an arc: an arc and the line across
    its ends enclose a D, as two arcs make a circle)
    closes the outline; `Enter`, **Done** or a double-click finishes it
    open; `Backspace` takes back the last point; `Escape` discards the
    draft. While a **Length** or an **Angle** is typed, `Enter` takes
    the point the fields give instead, and the next `Enter` finishes.
    A two-point line is a line. An open path is drawn dashed and
    cut by Engrave or a Profile on the line, as an imported one is
    (phase 4, capability 57).
88. **Arc segments** (`A`). While a path is being drawn, `A` (or the
    tool bar's **Arc**) makes the next segment an arc, in PureCutCNC's
    two clicks: the arc's end, then a point the arc passes through. A
    typed **Radius** replaces the second click, the arc bulging to the
    pointer's side. `P` and `S` switch the next segment back to a line
    or to a curve. Armed from the tools, **Arc** is the same tool
    starting in arc mode, so a lone arc is three clicks and `Enter`.
    Three points on a line make no arc and the click is ignored, with
    the reason in the status bar. A typed radius under half the
    distance between the ends is refused at its field. An arc that ends
    on the first point closes the outline. Finishing while an arc waits
    for its second click drops that arc and keeps the rest.
89. **Spline** (`S`; PureCutCNC's spline tool). Each click adds a point
    the curve passes through; the curve between them is smooth
    (Catmull-Rom tangents, as PureCutCNC builds it), closes on a click
    at the first point and finishes open on `Enter` or a double-click.
    While drawing a Line, `S` makes the following segments curves. A
    curve is saved as its nodes and handles (ADR-0006) and cut as its
    outline flattened at the project's flatten tolerance.
90. **What a new shape is.** A drawn shape is a feature instance with
    its own definition, placed where it was drawn, named for its kind
    and the next free number (*Rectangle 1*, *Slot 2*, *Line 3*; a
    six-sided polygon is *Hexagon 1*, as PureCutCNC names them; a path
    is a *Line*, an *Arc* or a *Spline* when its segments are all of
    that kind, else a *Path*),
    selected when the tool is left, visible and unlocked. It goes at the
    end of the Design Tree, or, when a group holds the selection, in
    that group after its other rows. The tool bar's **Construction** switch (100) makes the
    shapes that follow construction geometry until it is turned off. A
    shape too small to be one (under 0.01 mm across, PureCutCNC's
    minimum) is not made, and the tool stays as it was.
90a. **A path carried on** (the maintainer's review, 2026-11-23: two
    open shapes drawn end to end are one shape, with no trip through
    Edit nodes). A path finished open (87 to 89) with an end on the end
    of another open path, within 1e-6 mm as a snap to that end or a
    typed point puts it, is not a new shape: the paths become one,
    their curves kept, and every further open path met at the chain's
    ends joins too. The shape is the path the drawn one starts on, or
    else the one it ends on: its name, its place in the Design Tree,
    its group, its placement and the operations that cut it. Where the
    chain's two ends come together it closes, also when that leaves two
    nodes with an arc or a curve between them (an arc and the line
    drawn across its ends: the maintainer's report, 2026-11-24). The
    step is one for Undo, named *Extend “name”*, or
    *Close “name”* when it closes. Only a shape that is one open path
    of its own is carried on, visible, unlocked, with no linked copy;
    and construction geometry only by a path drawn as construction
    geometry, a path to be cut only by one that is. A closed shape's
    corner and the middle of a path are not ends. Paths already apart
    are joined by Join (97), which takes a tolerance.
91. **Text** (`⇧T`). A click sets where the text's baseline starts
    (a second click moves it); the tool bar's **Words** field then
    takes the keyboard and the words, previewed as they are typed, and
    `Ctrl`/`⌘ Enter` or **Done** commits. `Escape` takes the click
    back, and with no click puts the tool down. The shape is named
    *Text 1*. Its sizes (84), which the tool keeps for the texts that
    follow and across sessions:
    - **Words**, one line or several (`Enter` in the field starts a
      line; PureCutCNC's text is one line);
    - **Font**: one of the bundled fonts, or a font the user has added
      (below), in a list that drops down, a style other than the
      regular one named with its family (*Inter*, *Inter Bold*);
    - **Height**: the height of a capital letter, in project units
      (default 10 mm or 0.4 in), so 12 mm lettering is 12 mm tall on the
      board whatever the font. It is measured on the font's own *H*; a
      round letter reaches a little over, as type does;
    - **Letter spacing** (extra space between letters, as a percentage
      of the height; 0 by default, the font's own spacing and kerning
      pairs applied first) and **Line spacing** (a percentage of the
      font's own line height, default 100);
    - **Lines**: Left, Centre or Right, for several lines.

    An outline font gives closed outlines, the counters of letters as
    holes, so a V-Carve or a Pocket of text does what it does for
    imported lettering; letters that overlap, as a script's do where
    they join, are one outline. A **single-line font** gives open paths
    for Engrave: *Simple Stroke*, PureCutCNC's own, with capitals,
    digits and `. - _ / ?`; a small letter is set as its capital. A
    character the font has no glyph for is drawn as the font's own
    missing-glyph mark (Simple Stroke's `?`) and said under the bar's
    fields.

    In Properties a text's Shape section has the same fields: the words
    are written when the field is left or on `Ctrl`/`⌘ Enter`, each
    change is one undo step, and the outline is laid out again where
    the text is, for its linked copies too. A text keeps its
    proportions: it scales evenly or not at all, and is edited node by
    node only once it is converted to a path.

    **Bundled fonts** are open-licensed (SIL OFL 1.1, and Apache-2.0 for
    one) and travel in the app; the design spec lists them with their
    licences. **Add font…** (the last row of the Font list) takes a
    `.ttf` or `.otf` file, checks that it is a font, copies it to the
    app's fonts folder in the user config directory and chooses it; the
    folder is read when the app starts. The app never lists the
    system's fonts (`docs/plan.md`, settled decisions). A font file the
    app cannot read is refused with the reason, and not copied. A
    variable font is used at its default instance.

    A project does not carry its fonts. The text's outline is saved, so
    the project opens, shows and cuts anywhere (ADR-0006); on a
    computer without the text's font its Properties say *Font “Name” is
    not on this computer: the outline is kept*, the fields are
    read-only but the Font list, and choosing another font there, or
    adding that one, makes the text editable again. Text on a curve, bold or italic made
    by slanting, and system fonts are out of scope.

### Editing shapes

92. **Edit nodes** (`N`, or a double-click on a path; PureCutCNC's
    sketch edit with Graphite's selection). The selected path shows its
    **nodes**, and the tool acts on them:
    - **Select**: click a node, `Shift`-click to add, drag a box round
      several (Graphite; PureCutCNC moves one node at a time), `Ctrl`/`⌘
      A` for all.
    - **Move**: drag a node, or the selected nodes together; the arrow
      keys nudge them as they nudge shapes. A dragged node snaps (82)
      and can be typed: the tool bar shows the one selected node's **X**
      and **Y**. Dragging a straight segment moves its two nodes.
    - **Arcs**: an arc segment shows a handle at its middle; dragging it
      changes the bulge, its ends staying put, and the tool bar shows
      its **Radius**. Moving a node next to an arc keeps the arc through
      its old middle point, as PureCutCNC does.
    - **Curves**: a curve's node shows its two handles. Dragging one
      keeps the other in line with it (a smooth node), each keeping its
      own length; `Alt`-drag moves it alone and makes the node a corner
      (Graphite's colinear toggle; PureCutCNC has smooth nodes only).
    - **Add a node**: double-click a segment, or click it with `Ctrl`/`⌘`
      held; the segment splits there and the outline does not change.
    - **Delete nodes**: `Delete` removes the selected nodes and bridges
      the gap with a straight segment (a curve where either side was a
      curve); an outline is never left with fewer than two nodes, or
      three when closed.
    - **Break** (context menu, `B`): a closed outline opens at the node;
      an open one splits there into two shapes. **Delete segment**
      (select a segment, `Delete`): a closed outline opens there, an
      open one splits in two. The second part is named as a copy is
      (*Line (2)*); what cut the path cuts the first part, and its tabs
      stay with the first part. A path that is one outline of several
      in its shape, or that has linked copies, stays one shape with an
      outline more, so its copies follow.
    - **Join ends**: dropping an open path's end node on its own other
      end closes it; on another open path's end (within 14 px,
      PureCutCNC's reach) joins the two into one shape, which keeps the
      dragged path's name and what cut it. The other path must be
      visible, unlocked and no linked copy.
    - **Segment kind** (context menu on a segment; Graphite's, absent
      from PureCutCNC): *Make Straight*; *Make Arc* (a curve becomes
      the arc through its middle point; a straight segment bows out by
      an eighth of its length, to be dragged from there); *Make Curve*
      (a line gets handles a third of the way along it; an arc becomes
      the curve that follows it, with a node at each quarter turn).

    Each edit is one undo step (*Move node of “Line 3”*; a drag is one
    step on release). `Escape`, `Enter`, `V` or a double-click on empty
    stock leaves the tool with the shape selected. A linked copy's
    siblings follow every edit, since the edit is the definition's. A
    parametric shape must be converted first (84), and the tool offers
    it. An imported outline is edited the same way: it is a path of
    many short straight segments, since curves are flattened at import.
93. **Fillet and chamfer** (ported from PureCutCNC's
    `applyLineCornerFillet` and `applyLineCornerChamfer`). In Edit
    nodes, with one or more corner nodes selected, **Fillet** (the tool bar, or
    the context menu) rounds each corner with an
    arc tangent to both sides, by the **Corner size** typed in the tool
    bar (the one size, kept as the Rectangle tool's is; PureCutCNC also
    drags the radius out from the corner, which is not ported);
    **Chamfer** cuts each corner back by the same size
    along both sides. Both need a corner between two straight segments
    and a size both sides can take; a corner that cannot take it is
    left as it is and named in the status bar, the others being cut in
    the same undo step. The result is ordinary
    geometry: an arc and two shortened sides. A whole shape selected
    with the Select tool takes **Shape ▸ Fillet Corners…**, which rounds
    every corner that can take the radius, the way a dogbone-free
    inside pocket is prepared. It takes one shape at a time, and a
    shape that keeps its sizes is converted first.
94. **Offset** (**Shape ▸ Offset…**, `⇧O`). With shapes selected, the
    tool bar (81) takes a **Distance**, a **Side** (**Outward**,
    **Inward** or **Both**; for open paths **Left**, **Right** or
    **Both** of their direction) and whether the **Original** is kept
    (**Keep**, the default, or **Replace**); the result previews on the
    stock as the values change. An offset keeps arcs as arcs and rounds outside
    corners with arcs, as a toolpath does (`dc-geometry`'s offset, the
    engine's own; PureCutCNC mitres its offsets and loses the arcs).
    Each selected shape is offset by itself; an offset that vanishes
    (inward by more than half the width) makes nothing and is said. The
    results are new paths named `<name> offset`, selected, each beside
    its source in the Design Tree, or in its place, with what cut it,
    when the original is not kept. A shape with a hole grows outward
    while its hole shrinks. The distance is as the shape is seen: a
    shape scaled to twice its size is offset by the distance typed.
95. **Weld, Subtract, Intersect** (**Shape** menu; PureCutCNC's Join
    and Cut, with the intersection it lacks). For two or more closed
    shapes: **Weld** makes their union one shape; **Subtract** removes
    every other selected shape from the first selected; **Intersect**
    keeps what all share. The originals are replaced by the result,
    which is a new path named for the first (`<name> weld`), with
    holes where the result has holes, and operations that targeted the
    first shape target the result. Curved edges come out as short
    straight segments within the flatten tolerance (the engine's
    boolean does; arc fitting at export restores arcs in the program).
    A result of nothing (no overlap to intersect, a first shape wholly
    taken away) changes nothing and is said. Shapes are read in the
    order they were selected. A weld of shapes that do not touch is one
    shape of several outlines. The result is placed as the first shape
    is, and its tabs stay with it.
96. **Trim** (the Trim tool; PureCutCNC's trim, with the interior case
    it leaves undone). With the tool armed, the part of an outline
    under the pointer, between the two nearest crossings with other
    outlines or with itself, is highlighted; a click removes it. A
    closed outline opens; an open one shortens or splits in two. A part
    with no crossing on either side is the whole outline, and the click
    deletes nothing: the status bar says *Nothing crosses this
    outline*. Trimming a parametric shape converts it to a path first,
    in the same undo step. An outline's own end that only touches
    another outline is no crossing. An open path trimmed in its middle
    is two shapes, the second named as a copy is, when it is one outline
    of its own with no linked copy; else its shape has an outline more.
97. **Join** (**Shape ▸ Join…**, `Ctrl`/`⌘ J`). Selected open paths whose
    ends meet, or lie within a **Tolerance** (0.1 mm by default, in the
    tool bar, 81), become one path, end to end, a straight segment bridging
    any gap; a chain whose two ends meet closes. What the DXF importer
    does to a file's loose entities, at the user's word. Paths that
    reach nothing are left alone and counted in the status bar. A shape
    takes part when it is one open outline of its own, unlocked and with
    no linked copy; the joined path has the first one's name, placement
    and operations. Paths join where they are seen to meet, however
    each is placed.

### Arranging and organising

98. **Copies.** *Duplicate* (`Ctrl`/`⌘ D`) is phase 4's linked copy.
    New in this phase:
    - **Make Unique** (**Shape** menu, context menu): the selected
      linked copies each get a definition of their own, so editing one
      no longer changes the others. A shape that is not linked is left
      alone; with no linked copy selected the row is off.
    - **Cut, Copy, Paste** (`Ctrl`/`⌘ X`, `C`, `V`; Edit menu, context
      menu). Paste puts the copies under the pointer when it is over
      the Viewport (their middle at the pointer; the context menu's
      Paste, where the menu was opened), else 10 mm off the originals
      and 10 mm further with each paste, as shapes of their own (not
      linked), selected, visible and unlocked. What was cut pastes
      first where it was. A pasted shape keeps its name when the
      project has none like it, else it is named as a copy is. The
      clipboard is the app's, not the system's, and survives opening
      another project in the same window, so shapes carry from one
      project to another. The status bar says what was copied. While a
      field is typed in, or text is selected in a label, the three keys
      are the text's.
    - **Alt-drag** copies what is dragged (a linked copy), leaving the
      original where it was (Graphite); both are drawn while the drag
      is under way.
    - **The context menu** of a shape on the Viewport is its Design
      Tree row's (57): a right-click selects the shape unless it is
      selected already, and the menu is Rename; Hide, Lock; Cut, Copy,
      Paste, Duplicate, Delete; Make Unique and Convert to Path where
      they apply; Zoom to; Add operation. Beside every shape it is
      Paste and Select All. In the Edit nodes tool it is the tool
      bar's buttons for the node or segment under the pointer, with
      Delete and Select All.
    - **Array…** (**Arrange ▸ Array…**, the tool strip's **Array**
      button, and a shape's context menu;
      PureCutCNC's feature distribution, grid and radial), which takes
      its values in the tool bar (81). **Grid**: Columns, Rows (2 and 1 at first), and
      the Spacing between copies along X and along Y, measured as a gap
      (the default; 20 mm or 2 in at first) or as a pitch; a spacing
      below nothing goes the other way. **Circular**: Count (the
      selection among them; 4 at first), the centre's X and Y (typed,
      or clicked on the Viewport with snapping), the Angle the copies
      are spread over (360° by default, positive anticlockwise; round
      a whole turn the last copy stops short of the first, over less it
      is at the angle's end), and whether the copies turn as they go
      round or are kept upright. The selection is the array's first
      cell and stays where it is. The copies preview as the values
      change; **Done** makes them linked copies of the selection, in
      one group (99) named `<name> array` that the selection joins, in
      one undo step, and leaves the whole array selected. An array is
      not remembered as an array: it is copies, as PureCutCNC's is. An
      array of one, copies that would lie on one another, a centre at
      the selection's own middle, and more than 1000 copies are refused
      at the field that mends them.
98b. **Origin** (the maintainer's review of 2026-11-23). A resize, a
    turn and a flip of a selection are made about its **origin**, one
    of nine points of its box: its corners, the middles of its sides,
    and its centre, which it is until another is chosen. Properties has
    an **Origin** row of nine dots laid out as the box is, shown while
    the selection can be changed, and the Viewport marks the origin on
    the selection. It governs a typed W or H, the Arrange turns and
    flips and the typed angle, a corner handle's drag and the rotation
    handle's; a corner that is the origin scales from the opposite
    corner, and a side handle keeps its opposite side whatever the
    origin. X and Y are measured to the box's bottom-left corner
    whatever the origin. It is an app preference, kept while the app
    runs, not project data. Pictures have no origin: they resize about
    their centre and do not turn.
98a. **Align and distribute** (Properties, under the placement fields,
    and the Arrange menu; PureCutCNC's, with Y re-derived for Y-up).
    **Align** Left, Centre, Right, Top, Middle, Bottom moves each
    selected shape so that side of its bounding box meets the same side
    of the selection's; with one shape selected, or with **To stock**
    on, the stock is what they align to (PureCutCNC has no align to
    stock). **Distribute** Horizontally or Vertically, three shapes or
    more, spaces them evenly between the two outermost, by equal gaps
    or by equal centre distances. Top is the highest Y. Locked shapes
    do not move and are named in the status bar; a locked shape still
    counts toward the box the others align to, so shapes can be aligned
    to one that is locked. Distribute spaces the unlocked shapes among
    themselves. One undo step each, named for the shapes it was asked
    of; shapes already in place make no step.
99. **Groups** (PureCutCNC's flat folders and its grouped folder, as one
    thing). The first build had **folders**, which filed shapes in the
    Design Tree, and **groups**, folders whose shapes were also
    selected together; the maintainer's review of 2026-11-23 made them
    one, the **group**, and nothing the user reads says folder.

    A group is a named row of the Design Tree with its shapes under it.
    It is made of the selection by **Arrange ▸ Group** (`Ctrl`/`⌘ G`),
    by **Group** in a shape's context menu, or by the Shapes header's
    **New group** button, which with nothing selected makes an empty
    one to drag shapes into; a group may hold one shape. It is named
    *Group 1* (a double-click or its menu renames it in place), folds
    its rows away, lists how many shapes it holds, and its eye and
    padlock act on every shape in it. Rows drag into, out of and
    between groups, and within the tree to reorder (the order is the
    file's and the drawing order), a line showing where they will go; a
    selected row takes the selection with it. Groups do not nest. A
    group's rows lie together in the tree and in the file: a shape that
    joins a group, by a drag, a copy, a paste or a drawing tool, goes
    after the group's other rows.

    A group's shapes act as one on the Viewport: clicking any of them
    selects all of them, as does a marquee that touches one, so they
    move, turn and scale together; a double-click reaches the one
    shape. In the Design Tree a click on the group's row selects its
    shapes, and a click on a shape's row selects that row alone. The
    group's menu has **Select Together**, ticked when the group is
    made; unticked, its shapes are selected one at a time and the group
    only files them. Either way it is one undo step.

    *Ungroup* (the group's menu, or **Arrange ▸ Ungroup**, `⇧ Ctrl`/`⌘
    G`, which dissolves every group that holds a selected shape) keeps
    the shapes. Deleting a group (its menu) asks nothing and deletes
    its shapes, in one undo step. A group goes with its last shape,
    deleted or dragged out; one that is empty because it was made so
    stays until it is used, listed after the shapes. Text converted to
    paths and an array's copies arrive grouped.

    The project file says the same: a group is an entry of `groups`, a
    shape's group its `group`, and *Select Together* the entry's
    `together` (104).
100. **Construction geometry** (PureCutCNC's construction role). Any
    shape can be marked **Construction** (the switch in Properties, the
    ticked row of the context menu and of the Shape menu, the tool
    bar's *Construction* choice while drawing, which stays as it is set
    for the shapes that follow and across the drawing tools): it is
    drawn thin and dashed in its own colour, snaps and measures like
    any shape, and is **never cut**: no operation offers it as a
    target, marking a shape construction takes it out of every
    operation's targets and every Inlay's boundary (the Warnings drawer
    says which operation is left with none, as deleting does), the
    artwork checks ignore it, and *Fit stock to shapes* ignores it.
    *Add operation* on a selection that holds construction geometry
    makes the operation for the other shapes, and says why not when
    there are none; *Add to* an operation and target editing pass a
    construction shape by. An open path that only construction shapes
    place raises no `open_profile` warning: it is not a path that was
    meant to be cut. Turning Construction off does not put a
    shape back in the operations it left. A project file that names a construction shape as a
    target is an error, and nothing is exported (S21). The tree shows
    its row with the construction glyph.

### Tracing and measuring

101. **Backdrops.** A picture (phase 4, capability 56) is what is
    traced over; this phase gives it what tracing needs:
    - **Opacity** (Properties, 10 % to 100 %): how strongly the picture
      shows on the Viewport. It is saved with the project and changes
      nothing a Photo V-Carve carves, so no toolpath is regenerated for
      it. A locked picture can still be faded.
    - **Scale by two points** (Properties ▸ *Set scale…*, for one
      picture): click two points on the picture and type the real
      distance between them in the tool bar; the picture is resized
      about the first point so that they are that far apart, its
      proportions kept, as one undo step. The points are taken where
      the pointer is, with no snapping; a third click moves the second
      point, and `Escape` takes the last point back. A locked picture
      is not scaled. PureCutCNC's calibration is a resize by reference
      with a typed factor.
    - A picture does not snap, and a locked one cannot be selected by a
      click or a marquee on the Viewport, so that drawing over it never
      grabs it; its row in the Design Tree still selects it.

    Rotating a picture stays out of scope (phase 3's decision): a
    crooked scan is straightened before it is added.
102. **Start from nothing.** The start screen gains **Draw from
    scratch** beside its two paths: a new blank project, opened on the
    Design step with the Draw popover open. The Design step's panel has
    **Draw** beside *Add drawing* and *Add picture*, opening the same
    popover, and its empty tree says *Draw a shape, or add a drawing*.
    The start screen is for a project nothing has been done to: one
    with no shape, picture or operation and no step to undo or redo
    (the maintainer's review, 2026-11-23). Deleting every shape of a
    drawing leaves the empty stock in view, to draw on again, with
    Undo to bring the shapes back; it does not go back to the start
    screen.
103. **Measure** (`K`; PureCutCNC's tape measure). Two clicks, each
    snapping as any point does, show the distance between them, its ΔX
    and ΔY, and its angle from +X, in the tool bar and beside a dashed
    line; after the first click the line follows the pointer. ΔX and ΔY
    are signed, from the first point to the second, and the angle is
    anticlockwise from +X, from 0 up to 360. The measurement stays until
    the next click starts another, or `Escape` leaves the tool. With
    one shape selected and the tool armed, the tool bar also shows that
    shape's **Length** (its outlines', every one) and, when they are
    closed, its **Area** (holes left out), both as the shape is placed.
    A measurement is not project data and is not saved.

### The file and the rest of the app

104. **Format 29** (ADR-0002, ADR-0006). The project file gains, with a
    migration that fills each for an older file: `definitions[].shape`
    (`null`: a plain outline), `instances[].construction` (`false`),
    `instances[].group` (`null`), `groups` (empty) and
    `bitmaps[].opacity_fraction` (`1.0`). `dreamcarve check` and `export` read a
    format 29 file as they read any other; the command line gains no
    drawing commands.
105. **Everything else follows.** A drawn shape is a shape: the Design
    Tree, Properties, Arrange, the operations' target pickers, tabs,
    the artwork checks, the preview and export treat it as phase 4
    treats an imported one. Editing a shape that an operation targets
    regenerates that operation (phase 4, capability 71). A placed tab
    on an edited outline moves to the nearest point of the new outline;
    a tab whose outline is gone is deleted with it, and the status bar
    says how many.
106. **Keyboard.** The keys of 81 to 103, with phase 4's, in the
    Keyboard chapter and beside each menu item. A tool's key arms it
    from anywhere outside a field. `Enter`, `Escape`, `Backspace`,
    `Tab` and `Space`-drag (pan) mean what 81 to 93 say while a tool is
    armed. Phase 4's bare keys keep their meaning under Select (`R`
    rotates, `L` locks, `H` hides, `F` fits).

## Out of scope

- Constraints and parametric dimensions: PureCutCNC's fixed-distance
  constraints, driving dimensions, named dimensions and persistent
  dimension annotations (`docs/plan.md`, post-1.0 "parametrics").
- Text on an arc or a path, vertical text, synthetic bold or italic,
  system font enumeration, fonts embedded in the project.
- Tracing a picture into outlines automatically (deferred with any
  tracer's licence review, `docs/plan.md`); rotating or mirroring a
  picture; colour pictures.
- Extend, lengthen and a general break-at-intersection; arrays along a
  path and scaled arrays (PureCutCNC has them; after this phase);
  remembered arrays that can be re-edited.
- Booleans that keep arcs and curves as such (results are polygonal, as
  the engine's are).
- Nested groups, layers with colours, per-shape line styles.
- Freehand drawing, a pen tool that places handles by dragging
  (Graphite's), B-splines, ellipse arcs, stars, three-point circles and
  rectangles.
- The system clipboard; SVG or DXF export of the design (phase 8).
- Regions and Add/Subtract feature roles (PureCutCNC's 3D model
  semantics; ADR-0003), a second face, rotary work.
- 3D of any kind (phase 6 and 7).
- The GUI tool-library editor the roadmap listed here: phase 4 built it
  (capability 64a).
- Any change to toolpath generation, warnings or G-code. The engine is
  as phase 3 left it.

## Acceptance tests

Numbered from 107, each traceable to a capability. "Draws" means the
tool is driven through the app core with pointer positions in project
millimetres, as the window drives it; "the file" is the `.dcv` the app
saves. Tolerances are `EPS_MM` (1e-6 mm) unless stated.

107. **Format 29** (104). Every format 28 fixture loads, migrated, with
     `shape: null`, `construction: false`, `group: null`, `groups: []`
     and `opacity_fraction: 1.0`, and exports the programs it exported before; a
     format 29 file saved by the app round-trips byte for byte; a file
     claiming 28 that already has `groups` is refused.
108. **Rectangle** (85, 84). Drawing a Rectangle from (10, 10) to
     (70, 40) saves a definition with `shape` rectangle 60 × 30, square
     corners, and a closed, anticlockwise outline of four lines whose
     box is those corners; with Rounded 5 the outline is four lines and
     four arcs of radius 5; with Chamfered 5, eight lines. `Shift`
     makes the second point (40, 40); `Alt` centres it on the first.
     Typing W 60 Tab H 30 Enter after the first click gives the same
     file as the clicks.
109. **Circle, ellipse, polygon** (85a, 85b). A Circle from (50, 50) to
     (60, 50) is two half-circle arcs of radius 10 centred there. An
     Ellipse 40 × 20 has every outline vertex within the flatten
     tolerance of the true ellipse and its box is 40 × 20. A Polygon of
     six sides, radius 10 to the corners, has six vertices 10 from its
     centre, the first toward the second click; to the flats, the
     middle of each side is 10 from the centre.
110. **Slot** (85c; ports `src/types/slot.test.ts`). A Slot from (20, 50)
     to (60, 50), width 12: four segments, line, arc, line, arc; the
     arcs are half circles of radius 6 centred on the two clicked
     points; the outline is closed and anticlockwise; at 45° the same
     holds. Setting Width to 8 keeps both centres; setting Length to 50
     keeps the slot's middle. An inside Profile with a 10 mm tool on the
     8 mm slot raises `tool_too_large`.
111. **Gear** (86; ports `gearProfile.test.ts`). The default gear on a
     40 mm outside diameter has 24 teeth, module 40 ÷ 26, a closed
     outline with 24-fold symmetry and no point outside the outside
     radius or inside the root radius; a bore of 6 adds a circle of
     diameter 6 as a second outline; the ported validation cases are
     refused with their reasons.
112. **Line, arc, spline** (87, 88, 89). Clicks at (0,0), (40,0),
     (40,30) and `Enter` save an open path of two lines; a fourth click
     on the first point saves a closed triangle. With `A` before the
     third click, then (40,30) and a through point, the second segment
     is an arc through that point; three points in a line add nothing.
     A Spline through four points saves `shape` path with cubic
     segments and an outline within the flatten tolerance of them,
     passing through the four points.
113. **Snapping** (82, 82a; ports `snappingHelpers.test.ts`). With the
     pointer 10 px from a rectangle's corner and 2 px from its side,
     the point taken is the corner; 20 px from the corner and 2 px from
     the side, the side's nearest point; each kind's candidate is found
     on a fixture of a rectangle, a circle and a crossing line; a kind
     turned off is not offered; with nothing in reach the pointer's own
     point is taken; `Ctrl`/`⌘` held takes the pointer's point; `Shift`
     turns (30, 3) from (0, 0) into (30.15, 0), its length kept; the
     grid's spacing is 2 mm at 5 px to the millimetre and 1 mm at 8,
     a pinned spacing whatever the zoom, and a point with nothing else
     near lands on it.
114. **Typed sizes** (83). For each tool, the fields of each step are
     those capability 83 lists; a pinned width follows the pointer in
     height only; `1/4 in` in a millimetre project is 6.35; a refused
     value marks the field and makes no shape; a typed X and Y place
     the first point.
115. **Parametric edits** (84). For each parametric kind, setting each
     size through Properties writes `shape` and an outline that equals
     the one generated from it, in one undo step that names the shape
     and the size; a linked copy changes with it; the size shown for a
     shape scaled ×2 is twice the drawn size, and typing it writes half.
     Stretching an unrotated rectangle to W 80 changes its width and
     leaves its corner size and its transform's scale alone; stretching
     a circle makes an ellipse; stretching a polygon is refused;
     stretching a linked copy is refused with its reason. Convert to
     Path leaves the outline equal and `shape` `null`.
116. **Text** (91). *DreamCarve* in each bundled outline font at height
     10: the outline's capital D is 10 mm tall within 0.05 mm; every
     profile is closed; the counters of `D`, `e`, `a` and `r`… nest
     inside their letters; two lines centre-aligned share a centre X;
     letter spacing 10 % widens the ten-letter line by 9 mm (nine
     gaps of a tenth of the height); Simple Stroke gives open paths only. The same text and
     sizes give the same outline twice. A project whose text names a
     font that is not there opens, shows the saved outline, refuses
     edits to the text with the reason, and exports the same program.
     Adding a font file makes it available; a file that is not a font
     is refused.
117. **Edit nodes** (92). On a rectangle converted to a path: dragging a
     corner node moves that vertex and nothing else; a box selects two
     nodes and they move together; adding a node at a side's middle
     leaves the outline's length unchanged; deleting it restores the
     four vertices; Break opens the outline at that node; deleting a
     segment of an open path makes two shapes. Dragging an arc's handle
     changes its bulge and neither end. Dragging a smooth curve node's
     handle keeps the other handle in line; `Alt` does not. Make Arc,
     Make Curve and Make Straight change one segment's kind and no
     node. Dropping an open path's end on its start closes it. Each is
     one undo step, and editing through a rotated, mirrored linked copy
     moves the definition's vertex to where the pointer was (ports
     `editInPlace.test.ts`).
118. **Fillet and chamfer** (93; ports `profileEdit.test.ts`). A 5 mm
     fillet at the corner of a 40 × 30 rectangle path shortens both
     sides by 5 and joins them with an arc of radius 5 tangent to both;
     a 5 mm chamfer joins points 5 mm along each side; a radius both
     sides cannot take is refused and named; a corner next to an arc is
     refused. Fillet Corners at 3 on an L-shaped path rounds all six
     corners.
119. **Offset** (94). A 40 × 30 rectangle offset 5 outward is a 50 × 40
     outline with four arcs of radius 5; inward 5 is a 30 × 20
     rectangle; inward 20 makes nothing and says so; an open line
     offset Both gives two lines 10 apart; Keep original off removes
     the source and moves its operations' targets to the result.
120. **Weld, Subtract, Intersect** (95). Two overlapping 40 × 40
     squares 20 apart: Weld is one outline of area 2400; Subtract, the
     first less the overlap, area 800; Intersect, the overlap, area 800;
     a circle subtracted from a square inside it gives a square with a
     hole. Two squares that do not touch: Intersect changes nothing.
121. **Trim and Join** (96, 97). Two crossing lines: a click on one
     arm removes that arm up to the crossing; a click on the middle part of
     a line crossed twice splits it in two; a click on a circle crossed
     by a line opens it between the crossings; a line nothing crosses
     is left. Join of two lines whose ends are 0.05 apart is one path
     of four vertices, a 0.05 mm segment bridging the gap; of four lines round a square, a closed
     outline; ends 1 mm apart are left alone at the default tolerance.
122. **Copies** (98). Make Unique on one of two linked copies gives it
     a new definition equal to the old; editing it leaves the other.
     Copy and Paste give unlinked shapes with equal outlines, in this
     project and in another opened after. A 3 × 2 grid array at pitch
     20 × 15 makes five linked copies at those offsets in a group;
     with gaps of 5 the pitch is the shape's size plus 5; a
     circular array of 6 over 360° about (50, 50) places the copies 60°
     apart, turned when asked (ports `featureDistribution.test.ts`).
123. **Align and distribute** (98a). Three rectangles: Align Left gives
     all three the least X of their boxes; Align Top the greatest Y;
     To stock aligns to the stock's sides; Distribute Horizontally by
     gaps makes the two gaps equal and leaves the outer two where they
     were; a locked shape stays and is named.
124. **Groups** (99). Group files the selection in a group, of one
     shape or several, and the Shapes header's button makes an empty
     one; rows drag into, out of and between groups and reorder the
     file; a click on a grouped shape on the Viewport selects the
     group, and with *Select Together* off selects that shape; Ungroup
     keeps the shapes and Delete takes them; a group goes with its last
     shape, and one made empty stays. All of it survives save and
     load.
125. **Construction** (100, S21). Marking the plaque's `hole` shape
     construction takes it from the Drill's targets in the same step;
     no Add menu or picker offers it; the artwork checks and Fit stock
     skip it; it still snaps. A file whose Profile targets a
     construction shape fails `check` and `export` with
     `construction_targeted` and writes no program.
126. **Backdrops and Measure** (101, 103). Opacity 40 % is saved and
     changes no Photo V-Carve program; scaling by two points 50 mm apart
     typed as 120 multiplies the footprint by 2.4 about the first point,
     proportions kept; a locked picture is not picked on the Viewport.
     Measure from (0, 0) to (30, 40) reads 50, ΔX 30, ΔY 40, 53.13°; a
     40 × 30 rectangle reads length 140 and area 1200.
127. **Edits reach the toolpath** (105, S21). On the plaque, moving a
     node of the outline regenerates the Profile and only it, and the
     new program follows the new outline; the placed tabs sit on the
     new outline; deleting a profile's last segment deletes its tabs
     and says so.
128. **Drawn equals imported** (105, S21). The test part (below) drawn
     in the app, saved, and exported by `dreamcarve export` gives the
     programs the app writes, byte for byte, on all six
     post-processors. The drawn part with every shape's sizes removed,
     which leaves the plain outlines an import makes, gives the same
     programs byte for byte. The same outlines imported from an SVG of
     the part give the same programs but for the project's name in a
     comment, move for move, each coordinate within the last decimal
     place a program writes: an import rounds a coordinate to a
     millionth of a millimetre, which can tip one that falls on a
     rounding tie. The V-Carve of the text is then the same carve (as
     many moves, in the same box, cutting the same length) with some
     strokes in another order, since that millionth is enough to change
     which of two equally near strokes it takes first.
129. **Tools and keys** (81, 106). Each tool's key arms it and `Escape`
     returns to Select; a drawing tool stays armed after a shape with
     its options; while a tool is armed a click selects nothing; the
     Draw button shows the last drawing tool; every key of 106 is in
     the Keyboard chapter and its menu item.
130. **The window** (81–103). Through the window's accessibility tree:
     the Draw popover lists the ten tools; arming Rectangle shows the
     tool bar with its prompt and W and H; two clicks on the Viewport
     add *Rectangle 1* to the Design Tree; the Shape section shows its
     sizes; a double-click on a path shows its nodes; the Snapping menu
     toggles a kind; *Draw from scratch* opens a blank project on
     Design. Pictures of every tool, the tool bar, the popovers and the
     node editor are in `docs/specs/phase-5/review/`.

## Safety requirements

S1–S20 apply unchanged. New in this phase:

- **S21 A drawn shape is cut by its saved outline.** Generators, checks
  and post-processors read a definition's `profiles` and nothing else of
  it: no size, font or curve reaches the engine (ADR-0006), so the
  program for a drawn shape is the program for the same outline
  imported (acceptance 128). The Viewport draws `profiles`, so what is
  seen is what is cut, also when `shape` and `profiles` disagree, which
  `check` reports as `shape_outline_differs`. Construction geometry is
  never cut: an operation that names one is an error and export writes
  nothing (acceptance 125). An edit to a targeted shape marks its
  operations stale until they regenerate, and export waits, as S20 says
  (acceptance 127). No drawing tool sets a depth, a feed or a speed.

## Test part

**The hook plate**, designed in the app with no import, by the quick
start "Your first design":

- Stock 180 × 100 × 12 mm; origin bottom-left, Z zero at the top.
- A Rectangle 160 × 80 mm with Rounded corners of 8 mm, centred on the
  stock: Outside Profile, through, four tabs.
- Two Slots, length 30 mm, width 6.5 mm, along X, drawn once and
  mirrored as a linked copy: Pocket, 6 mm deep.
- A Circle of 5 mm at one corner arc's centre, arrayed 2 × 2 at a pitch
  of 144 × 64 mm: Drill.
- A Polygon, six sides, 13 mm across the flats, on the plate's centre
  line: Pocket, 5 mm deep (a nut trap).
- Text *DREAMCARVE* in a bundled font, 10 mm high, centred above the
  slots: V-Carve.
- One construction line along the plate's vertical centre, used to
  align the text and the hexagon.

A real cut is optional, since machine output does not change
(`docs/plan.md`). **Bench check, the maintainer's:** the part drawn by
following the quick start, exported, and cut once.

| Item | Result |
| --- | --- |
| App programs equal `dreamcarve export` of the saved file (diff) | |
| Plate 160 × 80, slot width, hole spacing, hexagon across flats, measured | |
| Date, version, controller, sender | |

## Documentation deliverables

- Quick start: **Your first design**, the hook plate from a blank
  project to G-code, with screenshots from the build.
- User guide, new chapters: **Drawing shapes** (the tools, the tool
  bar, typed sizes, what each shape's sizes are), **Snapping and the
  grid**, **Text and fonts**, **Editing nodes** (nodes, arcs, curves,
  fillet and chamfer), **Changing shapes** (offset, weld, subtract,
  intersect, trim, join), **Copies, arrays and alignment**, **Groups
  and construction geometry**, **Tracing a picture**,
  **Measuring**.
- User guide, updated: The app (tools, the Draw menu, the tool bar),
  Placing shapes (linked copies and Make Unique, align and distribute),
  Images (opacity, scale by two points), Keyboard (generated),
  Project file (format 29), Warnings (the two new codes), Overview and
  the README (drawing is no longer "planned").
- `CONTEXT.md`: **Parametric Shape**, **Path**, **Node**, **Curve**,
  **Construction Geometry**, **Group**, **Snap**, **Tool**
  (the Viewport's, as against a cutting **Tool**: the term is **Drawing
  Tool**), **Tool Bar**, **Backdrop**; **Slot** and **Linked Copy**
  updated; **Feature Definition** gains its `shape`.
- Contributing: Crate map (`dc-text`; the tool model), Porting from
  PureCutCNC (what of its sketch layer is ported and what is not;
  Graphite as the second reference and its header), Licensing (fonts).

## Exit criteria

- [ ] Acceptance tests 107–130 green on macOS locally, then on Windows,
      macOS and Linux in CI when the branch reaches `main`.
- [ ] S21's equality test (128) covers all six post-processors.
- [ ] The hook plate drawn in the app with no import, its programs
      equal to the CLI's for the saved file.
- [ ] Quick start "Your first design" and every chapter above written,
      with screenshots from the build.
- [ ] `CONTEXT.md` updated; every user-visible string keyed.
- [ ] The maintainer's review of the working build, and the decisions
      under "Open questions" settled.
- [ ] `v0.5.0-alpha.1` for testers when the maintainer says; `v0.5.0`
      after their sign-off.

## Sub-phases

Delivered in this order on the branch `Phase-5`, each ending with the
gate green and the worklog updated; no tag between them. The design spec
has the slices.

| | Sub-phase | Capabilities |
| --- | --- | --- |
| 5.1 | **Drawing**: format 29, parametric shapes, the tool model, snapping and the grid, typed sizes, every drawing tool but Text | 81–90, 102, 104, 106 |
| 5.2 | **Editing**: Edit nodes, fillet and chamfer, offset, weld, subtract, intersect, trim, join | 92–97, 105 |
| 5.3 | **Organising**: Make Unique, cut, copy and paste, arrays, align and distribute, groups, construction geometry | 98–100 |
| 5.4 | **Text**: `dc-text`, the bundled fonts, user fonts, the Text tool | 91 |
| 5.5 | **Tracing and measuring**: backdrops, Measure | 101, 103 |
| 5.6 | **Release**: the hook plate, the quick start and the chapters, the review pictures, the exit-criteria audit | exit |

## Open questions

Decided here so the work can proceed; each waits for the maintainer.

| # | Question | Taken for now |
| --- | --- | --- |
| 1 | One format bump for the whole phase, or one per sub-phase as phase 3 did? | One, format 29, in the first slice: nothing is released between sub-phases. A field added after `v0.5.0-alpha.1` is published takes 30. |
| 2 | Which fonts are bundled? | The design spec's list: five OFL families chosen for carving (a sans, a serif, a slab, a script and a stencil) plus Simple Stroke. The list is the maintainer's to change. |
| 3 | A fuller single-line font? Simple Stroke has capitals and digits only. The Hershey fonts cover lower case, under their own permissive terms, which are not on `docs/rust-guidelines.md`'s licence list. | Simple Stroke only, until the Hershey terms are reviewed. |
| 4 | User fonts: copied into the config directory, or embedded in the project? | Copied; not embedded (a project then never redistributes a font). |
| 5 | Tool keys clash with phase 4's bare keys (`R` rotate, `L` lock, `T` tab tool). | Phase 4's keys stay; Rectangle is `M`, Line is `P`, Text is `⇧T`. |
| 6 | The roadmap's "join" read as both joining open paths (97) and welding closed shapes (95, with Subtract and Intersect, which phase 4's out-of-scope list sent here as "booleans"). | Both are in. |
| 7 | Extend, arrays along a path and scaled arrays exist in PureCutCNC and are left out. | Out, listed under "Out of scope"; any can be a later slice. |
| 8 | A click with no snap candidate: PureCutCNC discards it. | Taken at the pointer. |
| 9 | Paste makes independent shapes; Duplicate and Alt-drag make linked copies. | As stated in 98. |
| 10 | The roadmap lists the GUI tool-library editor in phase 5; phase 4 built it. | Struck from phase 5 in `docs/plan.md`. |
