# Phase 1: Headless MVP — Functional Spec

**Status:** accepted

## Goal

A hobbyist who draws in Inkscape or any SVG editor can turn a closed shape
into a part on a grblHAL or GRBL router using only the `dreamcarve` command
line: one Profile operation, with holding tabs, cut in depth passes, exported
as G-code they can trust.

## Users and scenarios

- **Sam** exports a coaster outline from Inkscape as SVG, runs
  `dreamcarve init coaster.svg`, opens the generated `coaster.dcv` in a text
  editor, fills in their stock thickness, tool, feeds, and tab count, runs
  `dreamcarve export coaster.dcv`, loads `coaster.nc` into their grblHAL
  sender, and cuts it.
- **Sam, later,** changes the depth per pass in the `.dcv`, re-exports, and
  the new program differs only in the number and Z of passes.
- **Alex** forgets to set a feed rate. `dreamcarve export` refuses and names
  the tool and the missing field. Nothing is written.

## Capabilities

1. `dreamcarve init <file.svg>` reads the SVG and writes a `.dcv` project
   containing the SVG's shapes as feature definitions and instances, a
   default stock sized to the drawing's bounds, a default origin, one
   placeholder tool, one Profile operation targeting all closed shapes, and
   the grblHAL post-processor embedded. The placeholder tool's feed, plunge
   rate and spindle speed, the stock thickness, and the Profile depth are
   unset (`null`): `check` reports them and `export` refuses until the user
   sets them (#64).
2. The user can edit the `.dcv` by hand: stock dimensions, origin, tool
   geometry and feeds, Profile side (inside, outside, on line), total depth,
   depth per pass, cut direction (climb or conventional), tabs (count, width,
   height, shape), safe Z, and which post-processor to use.
3. `dreamcarve export <file.dcv>` writes G-code for the embedded
   post-processor. grblHAL and GRBL are available. `--post <id>` switches the
   embedded post-processor before export and updates the file.
4. `dreamcarve check <file.dcv>` validates the project and prints the
   warnings and errors that export would raise, without writing G-code.
   `export`, and `check` when the program could be built, print the
   program's extents in machine coordinates and the program's units, as
   the program writes numbers: `X min..max, Y min..max, Z min..max` of the
   tool centre (Z min is the deepest cut; arcs count to their bulge), then
   X and Y widened by the largest exported tool's radius, where the
   cutter's edge reaches. Users place clamps from them (#62).
5. `dreamcarve posts` lists the available post-processors.
6. The exported program cuts each Profile target in depth passes from the
   stock top to the configured depth, retracts to safe Z between targets and
   passes, plunges at the plunge rate, cuts at the feed rate, and leaves
   holding tabs standing where configured.
7. Closed shapes nested inside other closed shapes are treated as holes of
   the outer shape for inside/outside determination, so a ring cuts as a ring.
8. Open paths in the SVG are imported but not targeted by the default Profile
   operation, and `check` reports them.
9. Units: the `.dcv` states its display units (mm or inch); G-code is emitted
   in the post-processor's units with the correct units command.

## Out of scope

Pocket, Drill, V-Carve, Engrave. DXF. Lead-in/lead-out, ramped entry, arc
fitting of flattened curves. Multiple tools in one program. Any GUI. Undo
exposed to the user (the history type exists but nothing drives it).
Clamps. Text. Stock from a shape (stock is rectangular).

## Acceptance tests

1. **Init round-trip.** `init` on `fixtures/coaster.svg` writes a `.dcv` that
   loads back identically and contains one definition with one closed
   profile of the expected bounds.
2. **Refuse missing feeds.** `export` on a `.dcv` whose tool has `feed: 0`
   exits non-zero, prints the tool name and field, and writes no file.
3. **Depth passes.** A 12 mm thick stock, 12 mm depth, 4 mm per pass yields
   exactly three cutting passes at Z = 8, 4, 0 relative to stock bottom, in
   that order.
4. **Uneven final pass.** 12 mm depth, 5 mm per pass yields passes at 7, 2,
   0: the last pass is shallower, never deeper.
5. **Never below bottom.** A depth greater than stock thickness (plus any
   explicit through-cut allowance) fails export with an error.
6. **Safe Z.** Every rapid move in the program is at or above safe Z in
   machine coordinates. The first motion is a rapid to safe Z.
7. **Plunge feed.** Every straight plunge (Z decreases, XY stays) carries
   the plunge rate. Every cutting move that changes XY carries the feed
   rate, including a ramp that descends along the path.
8. **Outside offset.** An outside Profile of a 100×50 mm rectangle with a
   6.35 mm tool traces a path whose bounding box is 106.35×56.35 mm.
9. **Inside offset.** Same rectangle, inside, yields 93.65×43.65 mm.
10. **Ring.** A 60 mm circle containing a 30 mm circle, outside Profile,
    cuts the outer circle on the outside and the inner circle on the inside.
11. **Tabs.** Four 8 mm wide, 3 mm tall tabs on a 90 mm circle: the final
    pass has exactly four segments raised to Z = 3 mm above stock bottom,
    each spanning 8 mm plus the tool diameter along the path, evenly spaced.
    Passes above 3 mm are unaffected.
12. **Climb vs conventional.** An outside climb cut on a counter-clockwise
    outline traverses clockwise; conventional traverses counter-clockwise.
    Inside cuts are the reverse.
13. **Post-processor switch.** The same `.dcv` exported for grblHAL and GRBL
    produces identical XYZ motion and differs only in header, footer,
    comment style, tool-change, and work-coordinate lines.
14. **Golden parity.** For each case in `tests/golden/profile-*`, the
    cutting moves at each Z level match PureCutCNC's exported G-code within
    0.01 mm after allowing a different start point on the loop.
15. **Inch project.** A `.dcv` in inch display units exports G20 and inch
    coordinates when the post-processor is set to inches, and G21 with
    millimetres otherwise. Internal geometry never changes.
16. **Stays on the stock.** The coaster with its instance moved to
    `e` = 80 fails `check`, naming `op1` and the overhang past the +X edge;
    the default coaster passes. `export` prints extents whose Z range is
    that of the program's Z words and whose X and Y ranges contain those of
    its X and Y words (#62).

## Safety requirements

- **S1** No emitted Z is below the stock bottom minus the operation's
  explicit through-cut allowance (default 0). Checked on every program in
  tests by parsing the emitted G-code, not by trusting the generator.
- **S2** Rapid moves occur only at or above safe Z.
- **S3** Every cutting and plunging move has an explicit, user-supplied feed.
  Export fails if any tool an enabled operation uses lacks feed, plunge, or
  RPM. With manual spindle control (`feed_speed.spindle_control: manual`)
  RPM is optional; feed and plunge are still required. A tool no enabled
  operation uses is a warning, and its unset values are not reported; they
  are never defaulted, so enabling an operation that uses it brings the
  errors back.
- **S4** Spindle is started with the tool's RPM, then held for the
  post-processor's spin-up dwell (default 3 s) before the first cutting move,
  and stopped in the footer. With manual spindle control the program sends
  no `M3`, `M5` or `S` word and no dwell. Before the first cutting move of
  each tool, after the tool change, it pauses with `M0` and a message to
  turn the router on (with the tool's RPM as a hint, if set); before each
  tool change and at the end it retracts to safe Z and pauses with `M0` and
  a message to turn the router off. The safety pass checks that each tool's
  first cutting move follows an `M0` issued at safe Z after the last retract
  and after the previous cut, and that an `M0` at safe Z follows the last
  cut. In both modes the program does not stop for the first tool: the
  user fits it and sets work zero with it before starting, and `export`
  says so (#57). Every later tool change retracts to safe Z, stops the
  spindle, and stops with `T<n> M6` and a message to fit the tool and set
  Z zero, then retracts to safe Z again, from the new zero, before the
  spindle or router starts. Only a post-processor whose tool change has
  `M6` (grblHAL) can change tools; with any other (GRBL), export of
  operations that use more than one tool fails, naming the tools. A program
  that changes tools first sets the controller's current tool with
  `M61 Q<n>`, because grblHAL skips an `M6` for the tool it holds. Every
  `M0` and `M6` is at safe Z, and the safety pass treats Z as unknown after
  it, so a spindle start, the next stop, and an XY rapid each need a
  retract after the last stop; the program retracts after the router-on
  pause too (#43, #57, #68).
- **S5** The program begins with the post-processor's units command,
  work-coordinate select and absolute distance mode (`G90`), so a
  controller in the wrong state is corrected. After every pause or tool
  change that more motion follows, it sets them again, with the header's
  other modal lines, because the user may change them at the controller
  during the stop (#55, #57).
- **S6** Machine origin is applied as a translation and axis mapping only.
  No mirroring occurs in export (ADR-0001). The safety pass reads each
  operation's moves back into project space and checks that they are its
  toolpath, move for move: a rapid is one or two rapids (Z first when
  rising, XY first when falling) ending at its end point; a straight cut
  is one `G1` ending at its end point; an arc is `G2`/`G3` moves turning
  the same way (after any mirroring axis map) about the same centre (from
  I/J; with R, the controller's arc midpoint lies on the arc), or straight
  moves whose ends lie on the arc in order and that cut inside it by no
  more than the flatten tolerance. A move to where the tool already is may
  be written as nothing. The only other motion in an operation is a
  retract straight up before its first move or after its last. Points
  compare within one unit in the last written decimal place (#60).
- **S7** The cut stays on the stock (#62). Each operation's swept XY
  extent, its cutting moves (arcs to their bulge) widened by its tool's
  radius, lies inside the stock rectangle `[0, width] × [0, height]`. A
  cut exactly along an edge is allowed. `dc-cam` checks this after
  generating each toolpath, an error naming the operation and the
  overhang in mm past each edge it crosses; the safety pass checks it
  again on the parsed program, within one unit in the last written
  decimal place. There is no setting that allows an overhang.
- **S8** Values that are valid but dangerous are reported, never changed
  (#67). Only enabled operations and the tools they use are checked. Export
  fails on a feed or plunge rate above 20 000 mm/min
  (`MAX_RATE_MM_MIN`), a Profile cut with a cutter other than a flat end
  mill (Profile offsets by half the diameter, which is wrong at any depth
  for a V-bit, ball-nose or engraving bit), an enabled operation with no
  targets, and a post-processor `line_number_increment` of 0. A step-down
  deeper than the tool's diameter, a plunge rate above the feed rate, a
  spindle speed above 60 000 RPM (`MAX_RPM`; GRBL and grblHAL clamp `S` to
  `$30`), and a tool diameter above 50 mm (`MAX_TOOL_DIAMETER_MM`) are
  warnings. Each finding names the tool or operation and is located at the
  value in the file.

## Test part

**Coaster.** 90 mm diameter circle, outside Profile.

| Item | Value |
| --- | --- |
| Material | Baltic birch plywood, nominal 12 mm, measured thickness entered in the `.dcv` |
| Tool | 6.35 mm (1/4") two-flute upcut end mill |
| Feed / plunge / RPM | 1500 mm/min, 500 mm/min, 18000 (adjust to the user's machine; record what was used) |
| Depth per pass | 4 mm |
| Through-cut allowance | 0.2 mm into a spoilboard, set explicitly |
| Tabs | 4, rect, 8 mm wide, 3 mm tall |
| Direction | climb |
| Safe Z | 5 mm above stock top |
| Origin | stock bottom-left, Z zero at stock top |
| Controller | grblHAL |

**Success:** the coaster releases cleanly after cutting the tabs, measures
90 ± 0.2 mm across two diameters, and the toolpath never touched the clamps.
Record the date, measured dimensions, and any deviation below.

**Cut record:** not yet cut.

## Documentation deliverables

- User Guide: Installing; Quick start "Your first cut from an SVG" (the
  coaster, end to end); The project file (`.dcv` reference); Profile
  operation; Tabs; Post-processors (grblHAL, GRBL); Safety: what DreamCarve
  checks and what it cannot.
- Contributing: Crate map updated with real modules; Porting from PureCutCNC
  with the Y-up checklist; Golden corpus how-to.

## Exit criteria

- [ ] All acceptance tests and safety tests green on all three OSes.
- [ ] Golden parity cases captured and passing.
- [ ] Test part cut and recorded above.
- [ ] Quick start published.
- [ ] `v0.1.0` tagged with binaries.
