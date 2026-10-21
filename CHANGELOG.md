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
  lines; every other case writes the same holes as plain moves.
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
- **Structured warnings.** Every warning from `check` and `export` has a stable
  code and names the operation and shape, printed as `code: message`. The
  Warnings chapter lists them all.
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
- **LinuxCNC, Mach3 and UCCNC definitions version 2** leave the tool length
  offset alone: the header no longer has `G49` ([#286]). The work zero you
  set with a tool length offset active includes the offset, so cancelling it
  at program start moved every Z by the offset, possibly deeper than the
  drawing. `init` writes version 2. A version 1 project keeps its bytes
  until `export --post <id>`.
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

### Fixed

- The CLI no longer panics when its output pipe closes, as in
  `dreamcarve check part.dcv | head`; it finishes its work and exits with its
  own status ([#216]).
- A smooth tab's down slope no longer descends faster than the plunge rate
  ([#157]).
- Open paths now take part in the `toolpaths_overlap` and
  `cut_inside_freed_part` warnings ([#206]).

### Safety

New checks in the safety pass, which every exported program must pass before
it is written:

- **S9:** every cut stays in material its own operation removes. A Pocket stays
  inside its region and off its islands; an entry, lead or corner notch never
  enters the part.
- **S10:** a ramp or helix never descends faster than the plunge rate.
- **S11:** a canned-cycle line is expanded into the moves the controller makes,
  and that expansion must match the Drill toolpath move for move.
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
- Profiles that would cut through material you keep are refused (see Changed),
  and S9 holds hand-written programs to the same rules.
- Two enabled tools with the same `number`, or a number of 0, stop export.

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
[#286]: https://github.com/grayzu/dreamcarve/issues/286
[#287]: https://github.com/grayzu/dreamcarve/issues/287
