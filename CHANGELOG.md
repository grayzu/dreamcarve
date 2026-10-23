# Changelog

All notable changes to DreamCarve are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
the rule in the roadmap: `0.N.0` marks the end of phase N, and fixes between
phases bump the patch number.

## [Unreleased]

Phase 2: every 2.5D operation except V-Carve, from SVG or DXF, for six
controllers. This section becomes `0.2.0` when phase 2 is tagged.

### Added

- **Pocket operation.** Clears everything inside a closed shape to depth, in
  passes. Shapes inside a pocket are islands and stay standing; a shape inside
  an island is pocketed again. Two clearance strategies, named as VCarve names
  them: `offset` (rings stepping in from the wall) and `raster` (parallel lines
  at an angle you choose, joined in a zigzag). Both end with a wall pass, first,
  last or none. Stepover is a fraction of the tool diameter (`init` and the tool
  library default to 0.4), and `allowance_mm` leaves material on the walls for a
  finishing pass.
- **Rest machining.** A Pocket can name an earlier Pocket in `rest_of` and then
  clears only what the larger tool could not reach.
- **Drill operation.** Drills at the centre of each circular shape: simple,
  peck with a full retract, peck with chip breaking, and dwell. Shapes that are
  not circles are skipped with a warning. On LinuxCNC and Mach3 you can turn on
  canned cycles (`use_canned_cycles`, off by default) to get `G81` and `G82`
  lines; every other case writes the same holes as plain moves. With a
  clearance height, a Drill of separately drawn circles is written as cycles
  from it, which `G98` returns each hole to ([#305]). One that also has a
  part with several holes, so it crosses at both the clearance height and
  safe Z, is written as one series per part from safe Z, each ended by
  `G80`, with the rise to the clearance height between them ([#314]).
- **Engrave operation.** Cuts along open and closed lines with the bit centred
  on the line, at a fixed depth, in passes. `alternate_direction` cuts an open
  line back and forth instead of retracting between passes.
- **Several operations per project**, cut in the order they are listed, never
  regrouped by tool. `check` warns `cut_inside_freed_part` when an operation
  cuts inside a part an earlier one has already cut free, and hints
  `tool_change_avoidable` when the same tool is used in two operations that are
  not next to each other.
- **Machining order** (`machining_order`) on Profile, Pocket and Engrave:
  finish each shape before the next (`full_depth_first`, the default), or cut
  each pass across every shape before going deeper (`pass_first`), for vacuum
  tables.
- **Entry** on Profile and Pocket: a straight plunge, a zigzag ramp at an angle
  (default 15°), or a helix. An entry that does not fit falls back (helix to
  ramp, ramp to plunge) with an `entry_fallback` warning. A ramp's or helix's
  descent never goes faster than the plunge rate.
- **Arc lead-in and lead-out** on Profile, with an overcut distance. A lead
  that would cut into the part or another shape is shrunk (`lead_shrunk`) or
  dropped (`lead_dropped`).
- **Corner relief** on Pocket and Profile: dogbone or T-bone notches in the
  inside corners a round bit cannot reach, so square parts fit.
- **On line Profile of open paths.** An open path can now be cut On line, with
  ramps and tabs, as VCarve's Profile "On" does ([#175]). Inside and Outside still
  skip open paths.
- **Left and Right Profile of open paths; project format version 10.** A
  Profile with `side` `"left"` or `"right"` cuts beside an open path, one bit
  radius to its left or right looking from its first vertex to its last, as
  VCarve's Profile "to the Left" and "to the Right" of an open vector do
  ([#207]). The cut ends square at the path's ends, ramps along its own path,
  takes tabs, and has no lead or corner relief. `direction` is the milling
  mode, as on a closed shape: climb keeps the part on the right of travel, so
  Left climb starts beside the first vertex and Right climb beside the last
  ([#287]). A closed shape in such an
  operation is skipped with `side_needs_open_path`, and a cut that would cross
  another of its lines or reach the part side of one stops the export. A
  version 9 file loads unchanged; older DreamCarve versions cannot read
  version 10 files.
- **One program with tool changes, or one per tool.** Set your default once in
  the user config file (`config.toml`, `[export] tool_change_mode`), which
  `init` and `export --post` copy into new projects. Override it per export with
  `--one-program` or `--programs-per-tool` without changing the project. One
  program needs `M6`, which of the six post-processors only grblHAL has.
  `dreamcarve config path` prints where the config file lives.
- **Tool numbers.** A tool can carry a `number`; export writes it as the `T`
  word and in program file names (`plaque-1-T3.nc`). Without one, the tool's
  place in the list is used, as before.
- **Tool library** in your config directory: `dreamcarve tools list`,
  `tools add <project.dcv> <tool>`, `tools export <file>` and
  `tools import <file>`. Import reads DreamCarve JSON and Fusion `.tools` and
  `.json` libraries, following Autodesk's published schema, and reports each
  tool as imported, skipped (with the reason) or already present. Projects keep
  their own copy of each tool, so editing the library never changes a project.
- **DXF import.** `dreamcarve init part.dxf` reads LINE, ARC, CIRCLE,
  LWPOLYLINE, POLYLINE (with bulges), SPLINE, ELLIPSE, INSERT and MINSERT,
  ASCII or binary, with older files' code pages. Arcs stay arcs. Units come from
  `$INSUNITS`, or `--units` when the file has none. Layer names become shape
  names, `--layer` imports only the layers you name, and `--join-tolerance`
  joins line ends that meet. Entities DreamCarve does not read (TEXT, MTEXT,
  DIMENSION, HATCH and others) are counted in a `dxf_entities_skipped` warning.
- **Four more post-processors:** LinuxCNC (`.ngc`), Mach3 (`.tap`), UCCNC
  (`.txt`) and generic RS274/NGC (`.nc`). Each line of each definition was
  checked against the controller's documentation, and the definitions' README
  cites the source. Every controller gets the same tool motion; only the dialect
  lines differ.
- **Arc fitting at export.** Runs of short straight moves that lie on a circle
  within `arc_fit_tolerance_mm` (default 0.01 mm) are written as `G2`/`G3`
  arcs, so a curve exports as a few arcs instead of hundreds of lines. Set it to
  `null` to turn fitting off.
- **Placed tabs.** A tab is now part of the project, placed on one outline of
  one shape with its own width, height and shape, and it moves with its shape
  ([#105]). Every enabled Profile that cuts that outline rises over it.
  `dreamcarve tabs place <dcv> <op> --count N` places tabs with the rules of
  the tab setting (none on holes unless `--on-holes`, fewer on a short
  outline), `--at d1,d2,…` puts them at distances along each outline,
  `tabs list` shows every tab and the Profiles that cut round it, and
  `tabs clear` removes them (`tabs clear <dcv> <op>` the tabs one Profile cuts
  round, naming any part a through cut then frees with no tab). A Profile cuts
  round a tab only when its cut goes below the tab's top. `check` warns
  `tab_not_crossed` for a tab no enabled Profile cuts round, and `tab_on_hole`
  for a tab that holds a hole's slug.
- **`tabs place --replace`.** Swaps the tabs an operation cuts round for the
  new ones in one step, printing each tab removed and placed ([#293]). Without
  it `tabs place` still adds, and now notes an outline that already had tabs.
- **Tabs far off their outline; `tab_on_hole` once.** `check` and `export`
  warn `tab_off_outline` for a tab whose point is farther from its outline
  than its width plus the widest bit that cuts round it, and `tabs list`
  shows each tab's distance in a new `OFF_MM` column ([#300]). `tabs place`
  warns `tab_on_hole` once for each tab it puts on a hole. `check` and
  `export` no longer repeat it for tabs placed there for that cut, such as
  those a version 8 file's `on_holes: true` request asks for.
- **Estimated run time.** `export` and `check` print how long each program,
  and all of them together, should take: feed moves at their feed rates
  (arcs by their length), dwells, and rapids at the rate you set in the new
  `[estimate] rapid_mm_min` of `config.toml`, all times `scale_factor`.
  Without a rapid rate the rapids' distance is printed instead of a guessed
  time. Acceleration and the time at pauses and tool changes are left out,
  and the line says so ([#280]). The estimate never changes a program.
- **Clearance, tool-change and park heights; project format version 12.**
  The project's `heights` sets a clearance height above the stock, for moves
  between parts, between operations, at a tool change and at the end (each
  operation's `safe_z_mm` stays the height between passes of one part, like
  Fusion's retract height); a tool-change height in machine coordinates,
  written as `G53 G0 Z…` before every tool-change stop and at the end of
  every program, on grblHAL, GRBL and LinuxCNC; and a park point the
  program ends over, like VCarve's Home position ([#283]). All three are
  `null` on upgrade and from `init`, which exports as before. `export` and
  the program's header say to home the machine when a program lifts with
  `G53`, and what keeps a lift outside the travel from moving: on grblHAL
  and GRBL, Z homing upward or soft limits on (`$20=1`), since where Z homes
  downward the lift would otherwise drive Z down; on LinuxCNC, homing, after
  which its soft limits always apply ([#308]). The definitions gain
  `work_coordinates.machine_command` (`"G53"` on every grblHAL, GRBL and
  LinuxCNC version, `null` on the others), added in place: no program's
  bytes change. LinuxCNC's `G53` was checked against its 2.9 interpreter
  source ([#306]). Mach3, UCCNC and generic still refuse the tool-change
  height: their manuals do not say what `G53` does with a tool length
  offset, and the definitions README gives each reason.
  Older DreamCarve versions cannot read version 12 files.
- **Structured warnings.** Every warning from `check` and `export` has a stable
  code and names the operation and shape, printed as `code: message`. The
  Warnings chapter lists them all.
- **Toolpath preview.** `export --preview part.svg` and `check --preview
  part.svg` draw every operation's toolpath over the stock and the shapes, at
  1:1 in millimetres: cuts in each operation's colour, rapids dashed, a heavy
  mark where the bit rises over a tab, and a circle the size of the bit with a
  tick in the cut direction where each pass starts, so you can see where
  `at_distances` tabs are measured from. `export` writes it with the programs,
  all or none; it is never written over the project, a program, or a file that
  is not an earlier preview ([#281]).
- **Inch input.** Any length or feed in a project or tool library file may be
  written with its unit: `"depth_mm": "0.5 in"`, `"1 1/4 in"`,
  `"feed_mm_min": "60 in/min"`, `"safe_z_mm": "5 mm"`. Inches are converted to
  millimetres exactly when the file is read, so the program is the one the
  millimetre numbers give; a bad unit is an error at its line and column. A
  command that saves the file writes millimetre numbers back ([#282]).
- **Profile last pass allowance; project format version 13.** A Profile's
  `last_pass` (`allowance_mm`, `thickness_mm`, `reverse`) is VCarve's "Last
  Pass Allowance": every pass but the last runs `allowance_mm` farther from
  the part, and the last pass cuts on the part's path at full depth, so it
  takes an even skin off the whole wall and leaves no step marks
  ([#284]). `thickness_mm` sets how deep that last pass is; `reverse` cuts it
  the other way round. Tabs, leads, ramps, helixes and corner relief work with
  it: the earlier passes rise over a slightly longer stretch round each tab,
  and the notches are cut from the last pass. The allowance must be more than
  0 and at most a third of the bit's diameter, on an Inside, Outside, Left
  or Right Profile, and `thickness_mm` at most the step-down. The last pass takes
  off at most the allowance (√2 times it in a square corner), except in a
  sharp corner, where it takes the wedge the earlier passes stop short of,
  up to a third of the bit ([#324]): triangles and stars cut inside, V
  notches cut outside and letters such as A, V and W are cut, not refused.
  A drawing the earlier passes cannot follow (a band, neck, slot, notch or
  hole that fits the bit but not the bit plus the allowance, a tip whose
  wedge is thicker than that, or a part that close to another) is refused,
  naming the shapes. A one-pass
  cut warns `last_pass_unused`. `last_pass` is `null` in
  `init` and in upgraded projects, which cut exactly as before. Older
  DreamCarve versions cannot read version 13 files. Left and Right
  Profiles take a last pass too ([#311]): the earlier passes run the
  allowance farther to the cut side and end on the same lines through the
  path's ends as the last pass, so none cuts past an end and the skin is
  taken off right up to each; `reverse` starts the last pass at the other
  end. The earlier passes must keep the bit's radius plus the allowance
  from every other line and the strip it keeps. On an outline or an open
  path, the earlier passes now also rise over the thin edge of a tab where
  the last pass rises onto it, at the far side of its cut.
- **Tabs off corners and the Profile start point; project format version
  14.** A tab request's `avoid_corners` slides each tab that sits where the
  bit's path turns 30° or more within one bit diameter to the nearest place
  off it, at most half way to the next tab and never onto it; one with
  nowhere to go stays and warns `tab_on_corner`, so no tab is lost
  ([#285]). `tabs place --count` does the same unless `--allow-corners`.
  A circle more than about 1.91 bit diameters in radius has no corner, so
  the coaster cuts as before. A Profile's `start` (`"auto"`,
  `"first_vertex"`, or `{ "near": { "x_mm", "y_mm" } }`, VCarve's "Use
  vector start point" and "Start At") starts each closed outline's cut at
  its bit path's point nearest the drawn first vertex or that point: a
  request's tabs count from there, a lead arrives there or is dropped, and
  an entry with no room there falls back instead of moving. `init` writes
  `avoid_corners: true` and `start: "auto"`; upgraded projects get
  `false` and `"auto"`, and cut exactly as before. Older DreamCarve
  versions cannot read version 14 files.
- **Hole tabs that hold their slugs; project format version 15.** Each placed
  tab records in `holds_slug` whether it is meant to hold the slug of the hole
  it is on ([#302]). `tabs place --on-holes`, with `--count` or `--at`, and a
  request with `on_holes` write `true` for the tabs they put on holes, and
  `check` and `export` no longer warn `tab_on_hole` for those, even when they
  sit on the hole's outline or the Profile's bit or side has changed; any
  other tab on a hole still warns on every run. `tabs list` shows the flag in
  a new `HOLDS_SLUG` column. Upgraded projects get `null`, which warns as
  before, and cut exactly as before. The flag changes no cut. Older
  DreamCarve versions cannot read version 15 files.
- **Post-processor end-to-end plan and motion check.** `docs/testing/post-processor-e2e.md`
  says, for LinuxCNC, Mach3, UCCNC and generic, what is checked automatically,
  what cannot be and why, and the manual and air-cut procedures for the rest.
  `tools/rs274-motion` (dev only) runs those programs through LinuxCNC's
  `rs274` interpreter and compares its moves, feeds, dwells, canned cycles and
  `G53` lifts with DreamCarve's own reading, line by line ([#299]).
- **Controller run form.** A GitHub issue form, "Controller run", records one
  manual run of a program on a controller's demo or simulation mode or on a
  real machine: the controller and version, the DreamCarve version, the
  program and project, and the result of each check in the plan's procedure.
  The post-processors chapter lists a controller as not yet checked on a real
  machine until a passed real-machine run is recorded with it ([#330]).
- **Warning for a last pass round a curve drawn as lines.** A Profile's last
  pass slows down round true arcs, but not round a curve flattened to many
  short straight lines: a DXF ellipse or spline, or an SVG curve that is not a
  circle. Where such a curve bends so tightly that the bit's cutting edge
  moves more than 1.5 times the feed (in a 12 × 8 mm ellipse cut Inside with a
  6.35 mm bit, about 1.9 times), the operation warns
  `last_pass_flattened_curve`, naming the shape and where. Feeds and G-code
  are unchanged. Lower the feed, use a smaller allowance, or redraw the curve
  with arcs ([#350]).
- **Documentation:** a quick start, "A pocketed plaque from DXF", and chapters
  for Pocket, Drill, Engrave, machining order, importing DXF, the tool library,
  configuration and warnings. The Profile, post-processor, project file and
  safety chapters cover the new settings.

### Changed

These change what a project from 0.1 exports, or how DreamCarve behaves.

- **Shapes in one operation are cut nearest-first.** Shapes that are not nested
  are visited nearest-neighbour, starting from the first shape drawn, instead of
  strictly in drawing order (research decision D5, [#127]). A shape inside a hole
  is still cut before the hole, and ties keep drawing order ([#215]). A
  one-shape operation exports exactly as before.
- **One tab alone no longer warns `tabs_overlap`.** A single tab whose span
  covers its whole tool path warns only `tabs_cover_most_of_contour`; before,
  it warned that tabs overlapped as well ([#105]). The program is the same.
- **`init` writes a 15° ramp entry** for its Profile ([#119]). Projects from 0.1
  keep `entry: "plunge"` when upgraded, so they cut as before.
- **Ramps, helixes and straight plunges start 1 mm above the stock.** The bit
  rapids straight down from safe Z to 1 mm above the stock top (or 1 mm above
  the floor a previous pass cut), then descends at the plunge rate. Before, it
  fed down at the plunge rate all the way from safe Z, which was slow and cut
  air ([#214], [#227]). This also applies to upgraded 0.1 projects.
- **Drill pecks are counted from the retract height**, as a controller's
  `G83`/`G73` counts them. With `retract_mm` above 0, each peck ends that much
  higher and a hole may take one more peck: 12 mm in 4 mm pecks with a 1 mm
  retract is four plunges instead of three. Hole depth is unchanged ([#153]).
- **SVG circles and circular arcs are imported as arcs**, not as many short
  lines: circles, round ellipses, rounded-rectangle corners and circular path
  arcs, under any transform that keeps a circle round ([#70]). Their programs are
  shorter and the toolpath follows the exact circle.
- **Arc fitting is on by default.** `init` writes `arc_fit_tolerance_mm: 0.01`,
  and an upgraded 0.1 project gets the same value, so its programs use `G2`/`G3`
  where curves allow and get much shorter. The tool stays within 0.01 mm of the
  path it cut before. Set `arc_fit_tolerance_mm` to `null` for the 0.1 output.
- **grblHAL and GRBL definitions version 2.** The first header line is now
  `( Project: <name> )` on grblHAL and `; Project: <name>` on GRBL, so a project
  name can never start a controller command such as `( MSG, ...)` ([#171]). `init`
  writes version 2. A version 1 project keeps exporting version 1 bytes until
  you run `export --post <id>`, which upgrades it and says so.
- **grblHAL and GRBL definitions version 3.** The header, and the setup
  written again after every pause and tool change, cancel a canned-cycle
  motion mode (`G80`) and then cutter compensation (`G40`) after `G94`, so a
  mode left by an earlier job or by MDI cannot change the program ([#279]).
  `G49` is not added: both controllers fold an active tool length offset into
  the work zero you set, so cancelling it would move every Z. `export` now
  says so for grblHAL and GRBL programs. `init` writes version 3; a version 1
  or 2 project keeps its bytes until `export --post <id>`.
- **LinuxCNC, Mach3 and UCCNC definitions version 2** leave the tool length
  offset alone: the header no longer has `G49` ([#286]). The work zero you
  set with a tool length offset active includes the offset, so cancelling it
  at program start moved every Z by the offset, possibly deeper than the
  drawing. `init` writes version 2. A version 1 project keeps its bytes
  until `export --post <id>`.
- **Mach3 and UCCNC definitions version 3** cancel axis scaling (`G50`) and
  coordinate rotation (`G69`) in the header and in the setup written again
  after every pause ([#312]). Both controllers keep a `G51` scale and a `G68`
  rotation until something cancels them, and the end of a program does not,
  so a `G51 Z-1` left from MDI would mirror every Z and turn retracts into
  plunges. The safety pass accepts `G50` and `G69` only there, alone on a
  line, and refuses `G51` and `G68` on every definition. `init` writes
  version 3; a version 1 or 2 project keeps its bytes until
  `export --post <id>`.
- **grblHAL definition version 4** cancels axis scaling (`G50`) in the
  header, after `G40`, and in the setup written again after every pause and
  tool change ([#316]). grblHAL keeps a `G51` scale past the end of a
  program too. The safety pass accepts this `G50` under the same rule as
  Mach3's and UCCNC's. grblHAL has no rotation, so there is no `G69`. GRBL
  has no scaling and stays at version 3. `init` writes version 4; a version
  1, 2 or 3 project keeps its bytes until `export --post grblhal`. grblHAL's
  own `G50` leaves a scale whose X factor is 1 (such as `G51 Z-1`) in
  force, so soft-reset after any `G51` and check `$G`.
- **`export` warns grblHAL users about `G51`** ([#320]). Every export with
  a grblHAL definition whose header writes `G50` (version 4 and later)
  prints, once, after the tool length offset note: soft-reset after any
  `G51` and check that `$G` shows `G50`, because grblHAL's `G50` does not
  clear a scale that leaves X at 1. The note is printed only; no program
  changes.
- **On line Profile of an open path starts where VCarve's does:** conventional
  at the first vertex, climb at the last ([#242]). Tabs are still placed from the
  first vertex as drawn.
- **Profiles that would cut away material you keep are refused** with
  `BandNarrowerThanTool` or `PartNarrowerThanTool` instead of cutting:
  - an Inside Profile whose band (between an outline and an island, or between
    two islands) is narrower than the bit ([#192]); in 0.1 the bit gouged the part
    with no warning;
  - an On line Profile of a part whose hole is closer to its outline than the
    bit's diameter, or two holes that close ([#199]);
  - an On line open path that comes closer than the bit to its part's lines
    ([#210]);
  - a line cut On line that comes closer than the bit to itself, such as a
    hairpin or a star with sharp points ([#221]);
  - an On line Profile of a part so narrow that the bit removes all of it
    ([#223]).
- **Project format version 8.** Adds `arc_fit_tolerance_mm`, tool `number`,
  Profile `machining_order`, `entry`, `lead` and `corner_relief`,
  `post_processor.use_canned_cycles`, and the Pocket, Drill and Engrave
  operations. A version 7 file is upgraded in memory with values that keep it
  cutting as before (apart from arc fitting and the faster descent above), and
  is written as version 8 only when DreamCarve saves it (`export --post`).
  Older DreamCarve versions cannot read version 8 files.
- **Tabs per outline; project format version 9.** A Profile's tabs no longer go
  on its holes unless the new `tabs.on_holes` is `true`, so one operation cuts a
  part and its holes, holes first ([#109]). An outline whose tabs would cover
  more than half of its tool path gets the most tabs that cover at most half
  (a part keeps at least one; a hole may get none), and
  `tabs_cover_most_of_contour` says how many are cut; before, the tabs were cut
  as set. A version 8 file is upgraded with `on_holes: true`,
  so its holes keep their tabs.
- **Placed tabs; project format version 11.** Adds the project's `tabs`
  list, empty on upgrade ([#105]). A Profile's `tabs` setting stays as a tab
  request: it is cut exactly as before, and the first `dreamcarve tabs`
  command that saves the project writes it as placed tabs. `init` still
  writes its four tabs as a request. Once placed, two Profiles that cut one
  outline both rise over all its tabs; before, each rose over its own only.
  Placed tabs are never cut down on a short outline: they are cut as placed,
  with a `tabs_cover_most_of_contour` warning. Older DreamCarve versions
  cannot read version 11 files.
- `check` warns `cut_inside_tab_held_part` when an operation cuts deeper than
  half the stock inside a part that an earlier Profile through the stock holds
  only by its tabs, such as a hole cut after its tabbed part ([#269]). A
  lighter cut, such as a shallow engraving, stays quiet. A tabbed part used to count as
  held, so this order gave no warning.
- `export`'s summary states each operation's depth in the project's display
  units, with the post-processor's decimal places for them: `0.5000 in deep`
  in an inch project, `12.000 mm deep` (was `12 mm deep`) in a millimetre one
  ([#282]).
- **Names in comments keep their letters** ([#278]). Accented letters and
  common symbols in project, operation and tool names are spelled in ASCII
  instead of dropped: "Café Ø6 ½" is written `Cafe O6 1/2`, not `Caf 6`.
  A character DreamCarve has no spelling for is still dropped, and the
  characters controllers act on are still removed. This changes comment
  bytes only, on every post-processor and definition version, and only for
  names that hold such characters.
- **`export --post` keeps your settings** ([#277]). Switching post-processor,
  or upgrading to a newer version of your own, used to reset all six
  post-processor settings to the definition's defaults. Now a setting you
  changed, such as a longer `spindle_on_dwell_s`, is kept; one still at the
  old default takes the new one (so `nc` becomes `ngc` on LinuxCNC). A setting
  the new post-processor cannot use (`"m6"` without M6, canned cycles where it
  has none) is reset, and `export` names it, both values and why. Your
  configured `tool_change_mode` applies only to a reset mode. `--reset-settings`
  gives the old behaviour.

### Fixed

- A Profile's last pass no longer plunges through its allowance. It went
  straight down at its own start, where the allowance stands uncut to the
  stock top, as far as the earlier passes' floor before its ramp, or to the
  full depth with a plunge entry: up to 12 mm into material on a 12 mm cut in
  4 mm passes. It now goes down where the earlier passes ended, into their
  floor, with the operation's entry (a plunge one step-down deep, a ramp
  along their path inside their slot, a helix in their bore), then steps
  across the allowance onto its own path. No plunge goes deeper into uncut
  material than a step-down. A last pass lead-in that starts too far from
  the earlier passes' cut is left off with `lead_dropped` ([#339]).
- A Left or Right Profile with a last pass no longer fails export when its
  open path's end comes back beside its start. The two passes then start
  where they come within their own distance of that end, on two circles round
  it, so the last pass's step across the allowance runs a little off the
  normal, and the safety check refused it although it stays between the
  first segment's two passes, in material the operation cuts. The check now
  accepts a step between one segment's two offsets as well; a step past an
  open end or on the kept side still fails ([#358]).
- A Profile's last pass no longer overloads the bit in a small hole cut
  Inside. Its centre runs a much smaller circle than the wall, so at the feed
  its cutting edge moved up to 4.8 times as fast in an 8 mm hole with a
  6.35 mm bit, and took 3.6 to 4.1 times what a straight wall's last pass
  takes in a minute. Its inside arcs now run at the feed times the arc's
  radius over the wall's, rounded down, so the edge moves at the feed and
  the last pass removes at most a straight wall's share. The feed is never
  lowered below what the smallest hole the last pass cuts needs; an inside
  corner tighter than that warns `last_pass_edge_faster` ([#335]).
- The post-processors chapter no longer says that on LinuxCNC a `G53` lift
  outside the soft limits stops the program before the machine moves.
  LinuxCNC refuses the lift when it reads ahead to it, so the job can stop
  part-way, with the bit in the stock. The chapter now says so, and names
  the "Program exceeds machine limits" warning AXIS shows when you press
  Run, which catches it before anything moves ([#336]).
- A Left or Right Profile of an open path whose last (or first) segment is
  too short for the bit on the inside of the turn before it no longer cuts
  past the end. The cut beside the segment before it ran on past the line
  through the end vertex and took a sliver of the part at full depth: about
  0.05 mm² of the strip the path keeps for a 30° V with a 2 mm last segment
  and a 6.35 mm bit. Such a path is now refused, naming the vertex
  (`EndSegmentHasNoRoom`), with a last pass too when only the bit plus the
  allowance has no room; the strip a line keeps ends square at its end
  vertices, so a cut that reaches into it past an end is refused; and the
  safety pass's S9 holds every program to that strip ([#342]).
- A Profile whose cut starts under a tab no longer plunges into the tab on
  later passes. The move between passes came down to the previous pass's
  depth at the start, inside the tab, leaving it up to one step-down
  thinner, or more with a last pass. It happened with `start: auto` and a
  plunge entry, and with an explicit start with every entry. Each pass now starts on the tab at its height and gets
  down past the tab's end, and the safety pass's S9 refuses a plunge into a
  tab ([#337]).
- A Profile with a last pass and tabs no longer fails export when a pass
  before the last comes down off a tab in a slot, a hole or near a corner.
  The safety pass's S9 held those passes to the stretch the last pass rises
  over, its width plus the bit's diameter along the path, but they run
  beside the tab and come down where their bit stops touching the tab the
  last pass leaves, which is sooner where another stretch of the cut
  removes the far side of the tab's kerf. On a heightfield those plunges
  cut none of the tab. S9 now refuses such a plunge exactly when its bit
  would cut into that tab, so on a straight wall it also refuses one past
  the stretch that the old rule let through. No committed program changes
  ([#353]).
- An Inside or Outside Profile with a ramp entry and tabs no longer ramps
  into uncut stock on the passes below the tab top. Those passes placed
  their ramp beside the tabs on a line of its own, which no earlier pass
  had cut, so the first of them cut a slot up to a ramp's length (three
  bit diameters) into full-height stock at the cutting feed: 19 mm, about
  11 mm deep, for a 15 mm boss with a lead. Each pass now ramps along the
  line the pass above ramped on, cut short of the tabs, or plunges one step
  down at its start with `entry_fallback`. A last pass after its allowance
  passes, and a pass after one that plunged or went down by helix, plunge
  the same way, and so does a ramp that would go down less than 0.05 mm,
  whose one leg ran the whole line. Four committed programs change by one
  or two lines, each a ramp that left the trench ([#352]).
- An Inside or Outside Profile with a ramp entry no longer runs a later
  pass's ramp past the trench the passes above cut. The first pass starts
  its ramp 1 mm above the stock, so its first legs cut nothing, yet the
  next pass ramped the whole line: on a 1.33° ramp with a 0.415 mm
  step-down it ran 0.69 mm past the end of the first pass's trench, about
  twice the step-down into uncut stock. Each ramp now runs only as far as
  the one above it ran below the stock top, zigzagging within that, and
  the safety pass's S9 counts an earlier cut as trench only where it ran
  below the stock top. Before, S9 counted the whole cut, so the program
  exported with arc fitting off and failed with it on. The same fix lets
  an Inside Profile with a 16.7° ramp, a lead and a last pass export: its
  second pass had ramped 0.41 mm past the trench, 8.05 mm deep into uncut
  stock, and S9 refused it. No committed program changes ([#356],
  [#366]).
- The CLI no longer panics when its output pipe closes, as in
  `dreamcarve check part.dcv | head`; it finishes its work and exits with its
  own status ([#216]).
- A smooth tab's down slope no longer descends faster than the plunge rate
  ([#157]).
- A ramp, helix or smooth tab whose feed is lowered for the plunge rate no
  longer goes down up to 0.3% faster than the plunge rate once its numbers are
  rounded: its feed is rounded down until the move as written is at or under
  the plunge rate. A straight plunge whose rate the written places cannot hold
  is rounded down too ([#325]).
- Open paths now take part in the `toolpaths_overlap` and
  `cut_inside_freed_part` warnings ([#206]).
- An On line ramp whose leg ends a hair past a chord vertex no longer goes
  back to that vertex with a move too short to write. With arc fitting on,
  arc fitting counted that move as a chord the program does not write, and
  export failed S12 ([#321]).
- A fitted arc whose written centre strays from its chords is now written as
  the original lines. To make the rounded ends agree on the radius, export
  moves an arc's `I` and `J` by up to two units in the last place each way,
  0.0072 mm in inch. On a ring whose chords already bowed nearly the whole
  arc-fit tolerance, the arc as written came 0.0126 mm off a chord, past
  the 0.0125 mm S12 allows, and an Offset Pocket round a circular island
  failed export in inch. Export now checks each fitted run as written
  against the moves it replaces, as S12 does, before it keeps it. That
  check also counts the moves as S12 does: a move too short to write
  (0.0002 mm in a T-bone Pocket round a rectangular island) at the start
  or end of a run no longer makes up the three moves an arc must stand
  for. No committed program changes ([#355]).
- A T-bone or dogbone notch that would come nearer another wall than the bit's
  radius is no longer cut. A T-bone in a hole whose part is thinner past the
  hole's wall than the bit's radius cut through the part into the outside,
  and export failed S9. Such a corner is now skipped with
  `relief_corner_too_tight` and keeps the bit's radius ([#322]).

### Safety

New checks in the safety pass, which every exported program must pass before
it is written:

- **S9:** every cut stays in material its own operation removes. A Pocket stays
  inside its region and off its islands; an entry, lead or corner notch never
  enters the part. An Inside or Outside Profile's ramp or helix below the
  stock top runs only on the bit's path round the part or along an earlier
  cut of the operation where that cut ran below the stock top, unless it
  started at or above the stock top ([#352], [#356]).
- **S3, changed:** a Profile's last pass may run a level cut slower than
  the tool's feed, down to the feed times the allowance over the allowance plus
  the bit's radius, the speed its inside arcs run at in the smallest hole it
  cuts ([#335]).
- **S10:** a ramp or helix never descends faster than the plunge rate.
  Every descending cut is checked from the written numbers, with nothing
  allowed for rounding ([#325]).
- **S11:** a canned-cycle line is expanded into the moves the controller makes,
  and that expansion must match the Drill toolpath move for move. Rapids along
  one vertical line in one direction count as one move there, since cycles
  from the clearance height pass safe Z without stopping ([#305]). A series
  of cycles must end with `G80` before any other motion ([#314]).
- **S12:** a fitted arc replaces only moves it stays within the tolerance of,
  along their whole length, keeps the run's end points, sweeps at most 90°, and
  is dropped for the original lines if the controller would reject it after
  rounding ([#226], [#229], [#233]).
- **S13:** the cut-order rules (containment, the same cuts in every order,
  links at safe Z unless inside cleared material, deterministic order, operation
  order preserved, tabs kept) hold in both machining orders.
- **S14:** a rest Pocket is held to its own region; the earlier Pocket only
  shrinks what it clears.
- **S15:** a drill's rapid between pecks only goes down into depth already
  drilled.
- **S2, changed:** a rapid straight down from safe Z may stop 1 mm above the
  stock top, or 1 mm above the floor its own operation has already cut there,
  before a ramp, helix or plunge ([#214], [#227]).
- **S16:** a program writes `G53` only as `G53 G0 Z<v>` alone on its line, to
  the project's tool-change machine Z (below 0), on a definition that
  documents `G53`, from at or above safe Z, and only right before a
  tool-change stop or the end; work Z is unknown after it, so the next move
  across the work needs a retract first. Every other `G53` is refused
  ([#283]). S2 and S6 accept the rises to the clearance height and the move
  to the park point, which is held to the highest safe Z of the program.
- Profiles that would cut through material you keep are refused (see Changed),
  and S9 holds hand-written programs to the same rules.
- Two enabled tools with the same `number`, or a number of 0, stop export.
- The LinuxCNC and generic programs run through LinuxCNC's own interpreter,
  `rs274` 2.9.10, in CI (`gcode-conformance`), which fails on any error or
  warning it reports ([#173]).

### Removed

- The `dxf` crate dependency. DXF files are read by DreamCarve's own reader,
  which drops 51 crates from the build and also reads binary DXF and entity
  types the crate dropped ([#124]).

## [0.1.0] - 2026-10-20

Phase 1: cut a real part from an SVG with one Profile operation.

### Added

- `dreamcarve init <file.svg>` writes a `.dcv` project with one Profile
  operation; `dreamcarve export` writes G-code; `dreamcarve check` validates a
  project and reports problems without exporting; `dreamcarve posts` lists the
  post-processors.
- Profile operation: inside, outside or on the line, in depth passes, with
  rectangular or smooth holding tabs. Holes inside a shape are cut before the
  shape.
- grblHAL and GRBL post-processors, data-driven and ported from PureCutCNC,
  with spindle spin-up dwell, manual spindle control, and one program per tool
  run by default.
- The `.dcv` project format, versioned with upgrades for older files.
- A safety pass (S1–S8) that every program must pass before it is written:
  never below the material bottom, rapids at safe Z, explicit feeds, spindle and
  setup state, the program reproduces the toolpath, the tool stays on the stock.
  Export refuses a tool without a feed, plunge rate or spindle speed.
- A golden corpus comparing Profile output with PureCutCNC's.
- User guide and the quick start "Your first cut from an SVG". The phase 1 test
  part was cut on a grblHAL machine ([#17]).

[Unreleased]: https://github.com/grayzu/dreamcarve/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/grayzu/dreamcarve/releases/tag/v0.1.0
[#17]: https://github.com/grayzu/dreamcarve/issues/17
[#70]: https://github.com/grayzu/dreamcarve/issues/70
[#105]: https://github.com/grayzu/dreamcarve/issues/105
[#109]: https://github.com/grayzu/dreamcarve/issues/109
[#119]: https://github.com/grayzu/dreamcarve/issues/119
[#124]: https://github.com/grayzu/dreamcarve/issues/124
[#127]: https://github.com/grayzu/dreamcarve/issues/127
[#153]: https://github.com/grayzu/dreamcarve/issues/153
[#157]: https://github.com/grayzu/dreamcarve/issues/157
[#171]: https://github.com/grayzu/dreamcarve/issues/171
[#173]: https://github.com/grayzu/dreamcarve/issues/173
[#175]: https://github.com/grayzu/dreamcarve/issues/175
[#192]: https://github.com/grayzu/dreamcarve/issues/192
[#199]: https://github.com/grayzu/dreamcarve/issues/199
[#206]: https://github.com/grayzu/dreamcarve/issues/206
[#210]: https://github.com/grayzu/dreamcarve/issues/210
[#214]: https://github.com/grayzu/dreamcarve/issues/214
[#215]: https://github.com/grayzu/dreamcarve/issues/215
[#216]: https://github.com/grayzu/dreamcarve/issues/216
[#221]: https://github.com/grayzu/dreamcarve/issues/221
[#223]: https://github.com/grayzu/dreamcarve/issues/223
[#226]: https://github.com/grayzu/dreamcarve/issues/226
[#227]: https://github.com/grayzu/dreamcarve/issues/227
[#229]: https://github.com/grayzu/dreamcarve/issues/229
[#233]: https://github.com/grayzu/dreamcarve/issues/233
[#207]: https://github.com/grayzu/dreamcarve/issues/207
[#242]: https://github.com/grayzu/dreamcarve/issues/242
[#269]: https://github.com/grayzu/dreamcarve/issues/269
[#278]: https://github.com/grayzu/dreamcarve/issues/278
[#277]: https://github.com/grayzu/dreamcarve/issues/277
[#280]: https://github.com/grayzu/dreamcarve/issues/280
[#279]: https://github.com/grayzu/dreamcarve/issues/279
[#283]: https://github.com/grayzu/dreamcarve/issues/283
[#305]: https://github.com/grayzu/dreamcarve/issues/305
[#314]: https://github.com/grayzu/dreamcarve/issues/314
[#286]: https://github.com/grayzu/dreamcarve/issues/286
[#312]: https://github.com/grayzu/dreamcarve/issues/312
[#316]: https://github.com/grayzu/dreamcarve/issues/316
[#320]: https://github.com/grayzu/dreamcarve/issues/320
[#325]: https://github.com/grayzu/dreamcarve/issues/325
[#287]: https://github.com/grayzu/dreamcarve/issues/287
[#281]: https://github.com/grayzu/dreamcarve/issues/281
[#282]: https://github.com/grayzu/dreamcarve/issues/282
[#284]: https://github.com/grayzu/dreamcarve/issues/284
[#285]: https://github.com/grayzu/dreamcarve/issues/285
[#324]: https://github.com/grayzu/dreamcarve/issues/324
[#311]: https://github.com/grayzu/dreamcarve/issues/311
[#337]: https://github.com/grayzu/dreamcarve/issues/337
[#335]: https://github.com/grayzu/dreamcarve/issues/335
[#350]: https://github.com/grayzu/dreamcarve/issues/350
[#339]: https://github.com/grayzu/dreamcarve/issues/339
[#358]: https://github.com/grayzu/dreamcarve/issues/358
[#293]: https://github.com/grayzu/dreamcarve/issues/293
[#300]: https://github.com/grayzu/dreamcarve/issues/300
[#302]: https://github.com/grayzu/dreamcarve/issues/302
[#306]: https://github.com/grayzu/dreamcarve/issues/306
[#308]: https://github.com/grayzu/dreamcarve/issues/308
[#299]: https://github.com/grayzu/dreamcarve/issues/299
[#321]: https://github.com/grayzu/dreamcarve/issues/321
[#322]: https://github.com/grayzu/dreamcarve/issues/322
[#342]: https://github.com/grayzu/dreamcarve/issues/342
[#330]: https://github.com/grayzu/dreamcarve/issues/330
[#336]: https://github.com/grayzu/dreamcarve/issues/336
[#352]: https://github.com/grayzu/dreamcarve/issues/352
[#353]: https://github.com/grayzu/dreamcarve/issues/353
[#355]: https://github.com/grayzu/dreamcarve/issues/355
[#356]: https://github.com/grayzu/dreamcarve/issues/356
[#366]: https://github.com/grayzu/dreamcarve/issues/366
