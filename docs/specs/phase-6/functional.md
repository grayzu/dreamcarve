# Phase 6: 3D preview and simulation — Functional Spec

**Status:** draft, written 2026-11-23 ahead of the phase, while phase 5
waits for its tag; the maintainer reviews it before any slice starts.
Decisions waiting for them are under "Open questions". Written against
the `Phase-5` branch at `a2eee9b9` (format 29, capability 106,
acceptance 130, S21) and rebased onto it at `d75c61ef` on 2026-11-24,
when the work began; a slice amends it in the commit that changes what
it describes.
**Design spec:** `design.md`
**Decision record:** ADR-0003 (the 3D view is a heightfield simulation),
which this phase builds and does not change

## Goal

A hobbyist can see what the programs will leave on the stock before
anything is cut. A sixth step, **Simulate**, sits between Operations and
Export: it shows the stock in 3D, cut by every enabled operation in the
order the programs run, with each operation's own tool. The user can
turn the stock round, play the cut back with the tool moving, step
through it one operation at a time, and colour the result by operation
or by depth. One check is laid over the result: **uncut material**,
where the stock still stands above what the operations' shapes ask
for.

Nothing about toolpaths, warnings or G-code changes, and the project
file stays at format 29. The simulation reads the runs export writes and
changes none of them. Every safety rule S1–S21 holds; S22 says the
simulation is a reader.

PureCutCNC is the reference for the simulation itself
(`src/engine/simulation/`, `src/components/simulation/`), surveyed in
`docs/research/purecutcnc-simulation-digest.md`. Its simulation shows
what was removed and nothing else (digest, section 11): it plays one
operation at a time, colours nothing, and checks nothing against the
design. So this phase ports the grid, the cutter shapes, the playback
cursor, the renderer and the camera, and designs the rest here: playing
the whole job (113, 114), colour by operation and by depth (116), the
read-out (111), and the check for uncut material (119, 120).
Vocabulary follows `CONTEXT.md`, with the terms this phase adds listed
under "Documentation deliverables".

## Users and scenarios

- **Sam** is making the plaque and wants a 30 mm square block to sit in
  its square recess. *Continue* on Operations opens Simulate. The
  plaque appears as a block with its pockets, holes and engraved border
  cut into it and its outline cut through, the four tabs standing. Sam
  drags to turn it, presses **Top**, and turns on **Uncut material**:
  the four corners of the square recess light up, because a 1/4 in end
  mill leaves a 3.2 mm radius where the drawing has a sharp corner. The
  list under Result says *Square recess · 4 areas*. The block would not
  have dropped in. Sam goes back to Operations, sets the Pocket's corner
  relief to dogbone, and the list is empty.
- **Alex** has a logo to pocket with a 1/4 in end mill, and wonders
  whether the bit gets into its narrow necks and sharp inside corners.
  In the simulation the pocket's floor is flat where the bit ran, and
  **Uncut material** lights the necks and the corners it could not
  enter. Alex adds a second Pocket with a 1/8 in bit for the rest,
  presses **Play**, and watches the big bit clear the floor and the
  small one follow into the corners.
- **Jo** wants to know whether the holes are bored before the plaque is
  cut free. They click each row of the operations list in turn: the
  stock shows what has been cut up to and including that operation. The
  Profile is last, and the tabs are still there after it.
- **Sam, later,** runs `dreamcarve simulate plaque.dcv --out plaque.png`
  in a script that keeps a picture of every job's result beside its
  programs.

## Capabilities

Numbering continues from phase 5's capability 106 and acceptance test
130. "The user can" means in the app, on the Simulate step unless it
says otherwise. Nothing on the Simulate step changes the project, so
nothing there is an undo step.

### The Simulate step

107. **A sixth step.** The rail has **Simulate** between Operations and
     Export. On it the Viewport shows the stock in 3D (109) and the
     step panel holds, top to bottom: the summary line (*5 operations ·
     14 min 20 s*), the board choice when the project has an Inlay
     (118), the operations in cut order (114), **Result** (120–122) and
     **Display** (112, 116, 117). The step has no Inspector. Its state:
     *empty* until the project as it stands has been simulated;
     *complete* when it has. It never needs attention: nothing the
     simulation finds is a warning (120). Any change to the project
     clears the mark until the simulation is built again. *Continue* on
     Operations opens Simulate, and on Simulate opens Export. Nothing is
     locked: every step opens at any time, and Export never waits for a
     simulation (S22). The other five steps keep the 2D Viewport as
     phase 5 left it.

108. **What is simulated.** Every enabled operation's tool runs, in the
     order the programs cut them (the order the Export step's *Program
     Details* lists), each with its own tool, on the board it cuts. A
     carve's clearing passes and an Inlay's cut-out are runs like any
     other. Only cutting moves remove material; rapids and dwells remove
     none. The simulation is built when the step is opened and kept
     current while it is open. While toolpaths regenerate, or while the
     simulation is being rebuilt, the last result stays on screen,
     dimmed, with *Simulating…* and a progress fraction in the header's
     chip; it is replaced when the new one is ready. An operation with
     an error has no runs: the rest are simulated and the panel says
     *1 operation is left out: it has an error*, with *Show*. When
     validation refuses the project the Viewport shows the uncut stock
     and the panel says what blocks it.

109. **The stock in 3D.** The stock is a block of its width, height and
     thickness, standing on the **bed**, a flat surface at the stock's
     bottom that reaches a little past it on every side. The stock is
     one material colour, shaded by two fixed lights so walls, floors,
     slopes and scallops read at a glance, and a little darker the
     deeper it is cut, as PureCutCNC draws it. Where a cut goes
     through the stock the bed shows. Tabs stand where the Profile left
     them. The machine origin is marked with its X, Y and Z axes, as
     the 2D Viewport marks it with a crosshair. Both themes have their
     own colours; nothing is drawn as wood grain.

110. **Cutter shapes.** Each run cuts with the shape of its tool:

     | Tool | Shape the simulation cuts with |
     | --- | --- |
     | End mill | A cylinder of the tool's diameter with a flat end |
     | Ball nose | A hemisphere of the tool's diameter under a cylinder |
     | V-bit | A cone of its included angle up to the tool's diameter, a cylinder above |
     | Tapered ball nose | Its ball, its cone and its cutting diameter, as phase 3 defines them |
     | Drill | A cylinder with a flat end: the project holds no point angle |
     | Engraving bit | A cylinder with a flat end: the kind holds no shape |

     When a run uses a drill or an engraving bit, Result says so once
     (*2 tools are shown as plain cylinders*), so a flat-bottomed hole
     is not taken for the truth. The simulation invents no angle.

111. **Looking around.** A drag turns the stock about its centre; a
     drag with the right or middle button, or with `Shift`, slides it; the
     wheel or a pinch zooms about the pointer. The view cannot go under
     the bed. The floating pill at the Viewport's top left has **Home**
     (from the front right, above), **Top**, **Front**, **Right** and
     **Fit**. Top looks straight down with +Y up the screen, as the 2D
     Viewport does; Front looks from the operator's side. The view is
     in perspective. The read-out at the bottom left shows the
     X, Y and Z of the surface under the pointer, from the origin, in
     project units, and the name of the operation that cut there.

112. **Quality.** The stock is divided into square **cells**; each
     holds one height. Display ▸ **Quality** chooses how many: *Draft*,
     *Standard* (the default) or *Fine*, 512, 1024 or 2048 cells along
     the stock's longer side, never smaller than 0.02 mm. The cell size
     is shown beside the choice (*0.21 mm cells*). A wall's position is
     true to within one cell, and a depth is true at a cell's centre
     (acceptance 132). No grid has more than 16.8 million cells: the
     three qualities never reach that, and the CLI's `--cell` (125) is
     refused when it would.

### Playing the cut

113. **Playback.** A transport floats at the bottom centre of the
     Viewport: **Play** or **Pause**, a scrubber over the whole job, the
     time into the job and its length (*03:10 / 14:20*, from the moves'
     own feeds and the user's `[estimate]` settings), and **Speed**. While it plays the stock is cut as the tool
     reaches each move, and the tool is drawn where it is. Dragging the
     scrubber shows the stock as it is at any moment, backwards as well
     as forwards. Speed is a multiple of the programmed feed: 1×, 5×,
     25×, 100×, 500×; a job opens at the multiple that plays it in
     nearest to half a minute. Rapids take the time the user's
     `[estimate]` rapid rate gives them, or none when it is unset. At
     the end the stock is exactly the stock the step showed before Play
     was pressed (acceptance 139). Opening the step shows the finished
     stock, not the start.

114. **Operation by operation.** The operations list has one row per
     enabled operation in cut order: its colour, its name, its tool and
     its time. A click on a row shows the stock as that operation leaves
     it, with every operation before it cut; the rows after it are
     dimmed. **Previous** and **Next** on the transport step to the end
     of the operation before and after; with the stock at the end of an
     operation, Play continues from there. A row whose operation has
     several runs lists them under it (*Clearing · 1/4 in End Mill*;
     they are always shown, there being two or three at most), and a
     run can be clicked as an operation can. The row of the operation
     being cut shows its progress.

115. **The tool.** While the job is part-way through, the tool is drawn
     at its position in its cutter's shape, with a plain shank above the
     cutting length, in the theme's tool colour. It is not drawn at the
     very start or the very end. Display ▸ Layers turns it off.

### What is shown

116. **Colour.** Display ▸ **Colour** has three choices. *Material*: the
     one colour of 109. *By operation*: every cut cell takes the colour
     of the operation that cut it last, the colours the Operations list
     and the 2D Viewport give them. *By depth*: every cut cell takes its
     depth's colour on phase 4's depth ramp, with the depth legend
     shown. The uncut top keeps the material's colour in all three.

117. **Layers.** Display ▸ **Layers**: *Tool* (115), *Toolpath* (the
     moves of the operation being shown, as lines over the stock, cuts
     in the operation's colour), *Rapids* (its rapids, dashed), *Bed*
     and *Origin*. Toolpath and Rapids are off at first.

118. **The male board.** With an Inlay in the project, a two-way choice
     at the top of the panel, **Stock** and **Male board**, says which
     board is shown. Each is simulated by the runs that cut it. The
     male board is shown as it is cut, face up; fitting the plug into
     the pocket is not shown.

### Checking the result

119. **The design surface.** What 120 compares the stock with: the
     surface the enabled operations' shapes and depths ask for, as a
     perfectly sharp tool would leave it. It starts as the uncut stock
     and each operation lowers it:

     | Operation | What it asks for |
     | --- | --- |
     | Pocket | Its region, the targets less their islands, at its depth; its corner-relief notches with it |
     | Profile | A trench of the tool's width beside its outlines (Inside, Outside) or astride them (On), at its depth, less its tabs |
     | Bore | Its circles at its depth |
     | V-Carve | Inside its shapes, the depth a sharp V of the bit's angle reaches when it touches the nearest edge, no deeper than the flat depth |
     | Drill, Engrave, Inlay, Photo V-Carve | Nothing: what the operation's own runs cut is what it asks for |

     The design surface is never drawn on its own; it exists for 120.

120. **Uncut material.** Result ▸ **Uncut material** highlights, in the
     warning colour, every place where the finished stock stands above
     the design surface by more than the **threshold** (0.3 mm unless
     changed; 0.05 to 5 mm), and lists the places by operation with
     their number, their total area and their greatest height (*Square
     recess · 4 places · 5.1 mm² · up to 6.0 mm*; the corners are 8.7 mm²
     on paper, and a cell along each wall is the tolerance's). A click on a row frames
     its places from the Top. What it finds, among others: the corners a
     round bit leaves in a Pocket or an inside Profile, a Pocket too
     narrow for its tool, ridges between passes taller than the
     threshold. (A V-Carve too wide for its bit is not among them: an
     existing safety rule refuses it before anything is cut.) Tabs
     are not uncut material. A place narrower than about two cells is
     not found (the sliver a 1/8 in bit leaves in a hexagon's corner is
     a quarter of a millimetre wide): the panel says so beside the
     threshold, with the cell size. It is off until turned on, and it
     is not a warning: most projects have some.

121. **Gouges: not in this phase** (maintainer decision 2026-11-24).
     A gouge is a cut into material that was meant to stay. With no
     model of the part there is little for it to find: the safety pass
     already refuses a program whose operation cuts outside its own
     material (S9, S17). It moves to phase 7, where an imported model
     gives it its meaning. The number is kept so the ones after it
     stand.

122. **Result.** Above the check, the facts of the finished stock:
     the deepest cut and the operation that made it, whether and where
     the stock is cut through, the volume removed, and the line about
     plain cylinders (110). The same facts are the CLI's report (125).

### The rest of the app

123. **Never in the way.** The simulation is built on a worker thread
     and the window stays live while it is: the last result can still
     be turned and stepped through. *Stop* in the status bar abandons a
     build; the step then says *Simulation stopped* with *Simulate
     again*. A build that a change to the project has made out of date
     is abandoned by itself. After a change, only the operations from
     the first changed one are simulated again. Toolpath generation
     keeps its priority: a simulation waits for it.

124. **Preferences, and no format change.** Quality, Colour, Layers, the
     Uncut material switch, the threshold and the last Speed are
     preferences of the app, in `gui.toml`. The project file holds
     nothing of the simulation and stays at format 29.

125. **`dreamcarve simulate`.** `dreamcarve simulate <project.dcv>`
     prints the report of 122 with the uncut material list, and with
     `--out <file.png>` writes the finished stock as a 16-bit grayscale
     PNG, one pixel a cell, white at the stock top and black at the
     stock bottom. `--quality draft|standard|fine` or `--cell <mm>` set
     the cells, `--board female|male` the board (as `export` names
     them; the stock is the female), `--until <operation>` stops after
     that operation, and `--uncut-over <mm>` sets 120's threshold. It
     refuses a project `check` refuses, with
     `check`'s lines, and exits 0 whatever the check for uncut material
     finds. It writes `--out` over nothing or over a picture it wrote
     before, which it knows by the picture's own `Software` text, and
     over no other file, as `check --preview` treats its SVG.

126. **Keyboard.** `Space` plays and pauses; `[` and `]` are Previous
     and Next; `,` and `.` step one move back and forward (`Shift`: ten);
     `-` and `=` step Speed; `Home`, `7`, `1`, `3` are the views Home,
     Top, Front and Right; `F` is Fit. Step 5 on the rail is Simulate
     and Export becomes 6. Every key is in the Keyboard chapter and
     beside its menu item in **View ▸ Simulation**, a submenu, since
     eleven rows more made View too long for a short window. The keys
     are read on the Simulate step only, where the drawing's keys (the
     tools, Delete, the arrows) are not: they would change shapes that
     are not in view. `Space` is the Viewport's whenever the focus is
     only where a click left it; a control reached with Tab keeps it.

127. **Everything else follows.** Undo and redo work on the Simulate
     step and the simulation follows them. Units, themes, the Warnings
     drawer, the status bar and the header are as on every step. The
     start screen's examples and the quick starts open and simulate.

## Out of scope

- A model of the intended part, CSG, and any comparison against one
  (ADR-0003). The design surface of 119 is built from the operations'
  own shapes.
- Gouges, the cuts into material meant to stay (121): phase 7.
- A 3D view on the other steps, editing anything in 3D, and picking a
  shape or an operation by clicking the 3D stock.
- Undercuts, which a heightfield cannot hold and 3-axis 2.5D work does
  not make.
- Collision checks of the shank, the collet or clamps; a check of the
  cutting length against the depth of cut (the project holds a cutting
  length only for a tapered ball nose).
- Deflection, tear-out, chip load, heat: the simulation is geometry.
- Simulating a G-code file that DreamCarve did not just generate.
- Removing the offcut so only the part shows; fitting the male plug
  into the female pocket.
- A design surface for Inlay and Photo V-Carve.
- Simulating a chosen region of a large sheet at a finer cell.
- Exporting the result as a mesh (STL); wood grain and textures.
- Imported models, Surface Rough and Surface Finish (phase 7), which
  will draw their heightmap with this phase's renderer.
- Rotary work, a second face, round and square bar stock, plasma: all
  parts of PureCutCNC's simulation that DreamCarve has no setup for.
- Any change to toolpath generation, warnings or G-code.

## Acceptance tests

Numbered from 131, each traceable to a capability. "Simulates" means
through the engine with no window, as the CLI does. A **cell tolerance**
is one cell of the grid in X and Y; depths are compared at cell centres
within 0.001 mm.

131. **Cells** (112). A 200 × 100 mm stock at Standard has 1024 × 512
     cells of 0.1953 mm; at Draft and Fine, half and twice as many. A
     10 × 5 mm stock at Fine is held to 0.02 mm cells. `--cell 0.02` on
     a 1220 × 610 mm sheet is refused, with the cell that would fit.
132. **A flat end mill** (108, 110). One straight cut of a 6 mm end
     mill, 3 mm deep: every cell whose centre is within 3 mm of the
     move is 3 mm below the top, every other cell is at the top, and
     the same holds for a move at any angle.
133. **A ball nose** (110). The same cut with a 6 mm ball nose leaves
     the arc `z = tip + r − √(r² − d²)` across the move at every cell.
134. **A V-bit** (110). A 90° V-bit at 2 mm leaves `z = tip + d` out to
     `d = 2 mm`; one deeper than its cone leaves a vertical wall at its
     diameter.
135. **A tapered ball nose** (110). The cut's cross-section is the
     ball, then the cone, then the wall at the cutting diameter, at the
     heights phase 3's profile gives.
136. **Ramps, helices and arcs** (108). A move that falls as it goes
     leaves at every cell the lowest height the cutter's surface
     reaches over it anywhere along the move, for all four shapes; an
     arc is cut within 0.001 mm of the arc; a helix leaves a round
     hole.
137. **Plain cylinders** (110). A drill and an engraving bit cut as
     flat cylinders and the result names both tools.
138. **Through cuts and tabs** (109). The plaque's Profile leaves
     the cells of its trench at the stock bottom, which is *cut
     through*, except over its four tabs, which stand 3 mm high. No
     cell is ever below the stock bottom, and none ever rises. (The
     first draft named the coaster, which has no committed project.)
139. **Playback ends where the build ends** (113). For the plaque and
     the sign, playing from the start in steps of the whole job divided
     by 7, 53, 197 and 1000 ends with every cell equal to the direct
     build, and so does any sequence of seeks forwards and backwards
     that ends at the end.
140. **Part-way** (113, 114). At any time into a job, the stock is what
     a build of the moves up to that time leaves, with the move under
     way cut as far as the tool has gone; the tool's position is that
     point. A click on an operation's row gives the build of the
     operations up to and including it.
141. **The reference agrees** (108). For every fixture with a V-Carve
     or an Inlay, each cell of the simulation equals the exact
     per-point reference phase 3's tests use (`dc_cam`'s heightfield)
     within 0.001 mm.
142. **The runs export writes** (108, S22). The moves the simulation
     reads for a project are, run for run and move for move, the
     toolpaths `export` emits for it, in the programs' order, for one
     program for all tools and for one program a tool.
143. **The same everywhere** (S22). The same project and cell size give
     the same grid, byte for byte, on two runs and whatever the order
     the work is split in; on Windows, macOS and Linux every cell is
     within 0.0005 mm of the others' (a tool's angle goes through the
     platform's `tan`, whose last bit may differ).
144. **Changes** (108, 123). Changing the third of five operations
     simulates the third to the fifth again and reuses the first two;
     the result equals a build from nothing. Disabling an operation
     removes its cuts. Undo brings the earlier result back.
145. **Stop** (123). A build told to stop returns within one chunk of
     work and leaves the last finished result showing; a build made out
     of date by a change is dropped and its result never shown.
146. **The design surface** (119). For a fixture with one operation of
     each kind, the design surface at named points is the table's: a
     Pocket's floor inside its region and the top on its island, a
     Profile's trench less its tabs, a Bore's circle, a V-Carve's `depth
     = distance ÷ tan(α/2)` capped at the flat depth.
147. **Uncut material** (120). A 30 mm square Pocket with a 6 mm end
     mill reports four areas of `(1 − π/4)·r²` each within two cells'
     worth; with dogbone relief it reports none. A sliver one cell
     wide along a wall is not reported. The sign, V-carved with and
     without its clearing pass, reports none: without one the V-bit
     clears its own floor.
     Tabs are never reported. Ridges 0.25 mm high are not reported at
     0.3 mm and are at 0.2 mm.
148. **Only what is asked for** (119, 120). Nothing is reported where
     no operation's shape asks for a cut: beside a Profile's trench,
     under a Drill wider than its circle, along an Engrave, on an
     Inlay or a Photo V-Carve. The uncut stock reports nothing.
149. Withdrawn with 121 (it tested part and waste, which only gouges
     needed).
150. **The male board** (118). The lettered inlay's two boards simulate
     apart, each with its own runs and thickness; the female has no cut
     of the male's and the reverse.
151. **Colour** (116). After the plaque, every cut cell names the
     operation that cut it last; a cell cut by the Pocket and then by
     the Profile names the Profile.
152. **Looking around** (111). Top's view has +X to the right and +Y up
     the screen; Fit frames the stock and the bed in every preset with
     the same margin; the read-out over a Pocket's floor reads its
     depth and its name; no drag takes the eye under the bed.
153. **`dreamcarve simulate`** (125). For each of the coaster, the
     plaque, the sign and the hook plate, every pixel of the PNG is
     within one level of the committed one and the report's numbers
     equal the committed text's to the place they are printed; a file that is not its own picture is not replaced, and its own is; a project `check`
     refuses is refused with the same message.
154. **The earlier test parts** (exit). Measured on the simulation at
     Standard: the coaster is 90 mm across within a cell; the plaque's
     pockets, holes and outline are their sizes within a cell and their
     depths within 0.001 mm; the sign's lettering is as deep as phase
     3's carve facts say.
155. **The step** (107). A project with operations opens Simulate from
     *Continue* on Operations; the step is empty before a build,
     complete after one, and empty again after any change; it never
     needs attention; Export's state is what it
     was without the step.
156. **Nothing changes** (124, S22). With the simulation built, stopped
     or never opened, the saved project and every exported program are
     byte for byte the same; a project saved from the Simulate step is
     format 29 with no new field; the preferences round-trip through
     `gui.toml`.
157. **The window** (107–118). Through the window's accessibility tree:
     the rail has six steps; Simulate shows the operations in cut
     order; Play advances the time and Pause holds it; a row click
     dims the rows after it; Quality, Colour and Layers change what is
     drawn; the male board is offered only with an Inlay. A frame of
     the plaque rendered through wgpu is not blank and differs between
     the start and the end of the job. Pictures of every state are in
     `docs/specs/phase-6/review/`.
158. **Keyboard** (126). Each key does what 126 says and is in the
     Keyboard chapter and its menu item.

## Safety requirements

S1–S21 apply unchanged. New in this phase:

- **S22 The simulation is a reader.** It reads the runs `export` writes
  for the same project, in the same order, with the same tools
  (acceptance 142), and nothing reads it back: no generator, check,
  warning or post-processor takes anything from a simulation, so every
  program is byte for byte what it is with the Simulate step never
  opened (acceptance 156). Export does not wait for a simulation and is
  not blocked by what one finds. The simulation invents nothing: a tool
  whose shape the project does not hold is shown as a plain cylinder
  and named as such (110), and a playback time is the estimate's. The
  safety pass on the written program stays the only gate.

The book's chapter must say what the simulation is not: it does not
know the clamps, the machine, the material or the bit's real condition,
and a clean simulation is no reason to skip the first-cut precautions
of the Safety chapter.

## Test part

No new part: machine output does not change. The exit criterion of
`docs/plan.md` is that the simulation of each earlier test part matches
the physical cut. **Bench check, the maintainer's:** each part already
cut is laid beside its simulation at Standard quality, and the sizes
below are read from the read-out (111) and measured on the part. They
agree when they differ by no more than 0.3 mm across and 0.2 mm in
depth (one cell at Standard on these stocks is at most 0.22 mm, and the
earlier phases' bench tolerance is 0.2 mm), and when the corners a bit
could not reach have the bit's radius on both.

| Part | What is compared | Simulated | Measured | Date |
| --- | --- | --- | --- | --- |
| Coaster (phase 1) | Diameter; tab width and height | | | |
| Plaque (phase 2) | Raster pocket size and depth; dogboned pocket; hole diameter; engraved line width | | | |
| Sign (phase 3, as far as it was cut) | Raised oval's edge width; lettering depth at its widest stroke | | | |
| Lettered inlay pair (phase 3) | Female pocket depth; male plug height | | | |
| Line-art inlay, photo, tapered ball nose (phase 3) | When cut | | | |
| Hook plate (phase 5) | When cut: slot width, nut trap across flats | | | |

A part not yet cut does not hold the phase: its row is filled when it
is (open question 9).

## Documentation deliverables

- Quick start: **Check the cut in 3D**, the plaque from its project to
  a simulation, played back, with the square recess's corners shown
  uncut once its corner relief is turned off; the other
  quick starts gain the Simulate step between Operations and Export,
  with one picture each.
- User guide, new chapter: **Simulation** (the step, looking around,
  quality and what a cell is, playback, operation by operation, colour,
  uncut material and what it is compared with, the male board,
  what the simulation does not know, `dreamcarve simulate`).
- User guide, updated: The app (six steps), Toolpath preview (where 2D
  ends and 3D begins), Keyboard (generated), Safety (the paragraph
  S22 asks for), Configuration (`gui.toml`'s simulation keys), Overview
  and the README (simulation is no longer "planned").
- `CONTEXT.md`: **Simulation**, **Heightfield** (the simulation's grid;
  today a note under Heightmap), **Cell**, **Bed**, **Cutter Shape**,
  **Playback**, **Design Surface**, **Uncut Material**; **Step** gains
  Simulate; **Preview** is
  narrowed to the 2D toolpath preview.
- Contributing: Crate map (`dc-sim`), Porting from PureCutCNC (what of
  its simulation is ported, what is left out and why), Architecture
  (the simulation's job on the worker, the 3D renderer).

## Exit criteria

- [ ] Acceptance tests 131–158 green on macOS locally, then on Windows,
      macOS and Linux in CI when the branch reaches `main`.
- [ ] S22's two tests (142, 156) cover all six post-processors and both
      program splits.
- [ ] The bench check's table filled for every part that has been cut.
- [ ] Quick start "Check the cut in 3D" and the Simulation chapter
      written, with screenshots from the build.
- [ ] `CONTEXT.md` updated; every user-visible string keyed.
- [ ] The maintainer's review of the working build, and the decisions
      under "Open questions" settled.
- [ ] `v0.6.0-alpha.1` for testers when the maintainer says; `v0.6.0`
      after their sign-off.

## Sub-phases

Delivered in this order on the branch `Phase-6`, each ending with the
gate green and the worklog updated; no tag between them. The design spec
has the slices.

| | Sub-phase | Capabilities |
| --- | --- | --- |
| 6.1 | **The engine**: `dc-sim`, cells, cutter shapes, the cut, runs in order, playback, `dreamcarve simulate` | 108, 110, 112, 125, and 113–114 without a window |
| 6.2 | **The 3D view**: the Simulate step, the renderer, looking around, playback and stepping in the window, the tool, colour, layers, the male board | 107, 109, 111, 113–118, 123, 124, 126, 127 |
| 6.3 | **Checking the result**: the design surface, uncut material, the result's facts | 119, 120, 122 |
| 6.4 | **Release**: rendering refinements, the performance pass, the book, the review pictures, the bench check's table, the exit-criteria audit | exit |

6.1 ends with something usable: a picture and a report of any project
from the command line. 6.3 stands apart on purpose: it is the one part
with no reference to port, and the maintainer can reshape it without
touching 6.1 or 6.2.

## Open questions

Decided here so the work can proceed; each waits for the maintainer.

| # | Question | Taken for now |
| --- | --- | --- |
| 1 | A sixth step, or a 2D/3D switch on the Viewport of every step? | **Settled 2026-11-24: a step.** It gives the playback and the result a home, keeps 3D out of every editing gesture, and phase 4's `experience.md` left the rail as the seam for exactly this. A switch on the Operations step can follow if going back and forth is felt. |
| 2 | With no model of the part (ADR-0003), what do "uncut" and "gouge" compare the stock with? | **Settled 2026-11-24: uncut material only, against the design surface of 119** (each operation's own shapes and depths, sharp-cornered). Gouges wait for phase 7's models. The design surface is DreamCarve's own; PureCutCNC has nothing to port here. |
| 3 | Should uncut material be a warning code, in the Warnings drawer and `dreamcarve check`? | No, which the maintainer accepted on 2026-11-24 ("Ok... I think"), so it may be looked at again with the build. It depends on the cell size, costs seconds, and most projects have uncut corners. It is the Simulate step's own list and the `simulate` command's report. |
| 4 | Simulate the toolpaths, or the written program read back? | The toolpath runs: each move then knows its operation and its time. S11 and S12 already hold the written program to the toolpath within the tolerances, and acceptance 142 ties the two. |
| 5 | A drill has no point angle and an engraving bit no shape in the project. Draw a 118° point? | No: plain cylinders, said so in Result. A point angle on a drill would be a format change and its own decision. |
| 6 | Uncut threshold 0.3 mm. | As stated, and the user's to change. Phase 3 chose 0.25 mm ridges as acceptable on the sign, so the default must pass them. |
| 7 | Is `dreamcarve simulate` wanted? | Yes: it makes acceptance 153's goldens possible and gives 6.1 something to show before there is a window. |
| 8 | Quality as three levels by cell count. A 1220 × 610 mm sheet at Fine has 0.6 mm cells, too coarse for small lettering. | Three levels (PureCutCNC's slider runs from 240 to 1500 cells and starts at 280). Simulating a region at a finer cell is left out of this phase and named in "Out of scope". |
| 9 | The exit says every earlier test part's simulation matches its cut, and three of phase 3's parts and the hook plate are not cut. | **Settled 2026-11-24.** The parts that are cut are compared; the others when they are. The phase does not wait for them. |
| 10 | The view's projection. | Perspective throughout, as PureCutCNC's (45°). Sizes are read from the read-out, not by eye. An orthographic Top can follow if it is missed. |
| 12 | Rapids on the timeline when no rapid rate is set. PureCutCNC plays them at the cut feed. | They take no time and the tool jumps, as the estimate leaves them out: a made-up rapid rate would make the transport's length disagree with the Export step's. |
| 13 | PureCutCNC's two rendering refinements (walls that follow the tool's true edge, issue #973; the exact cut inside a cell, issue #1209) cost about 1,400 lines and a second pass over every move. | Not in the first build: walls stand on cell edges, and Standard is 3.6 times finer than PureCutCNC's default detail. Sub-phase 6.4 decides from the pictures whether #973 is ported. |
| 11 | Should opening a finished project land on Simulate (the first step that is not complete)? | No: a project opens on the step it opened on in phase 5, and Simulate is never the reason. Only *Continue* leads through it. |
