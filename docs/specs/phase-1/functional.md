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
   the grblHAL post-processor embedded. Placeholder values that must be set
   by the user are obviously invalid (feeds of 0).
2. The user can edit the `.dcv` by hand: stock dimensions, origin, tool
   geometry and feeds, Profile side (inside, outside, on line), total depth,
   depth per pass, cut direction (climb or conventional), tabs (count, width,
   height, shape), safe Z, and which post-processor to use.
3. `dreamcarve export <file.dcv>` writes G-code for the embedded
   post-processor. grblHAL and GRBL are available. `--post <id>` switches the
   embedded post-processor before export and updates the file.
4. `dreamcarve check <file.dcv>` validates the project and prints the
   warnings and errors that export would raise, without writing G-code.
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
7. **Plunge feed.** Every move that decreases Z carries the plunge rate.
   Every XY cutting move carries the feed rate.
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

## Safety requirements

- **S1** No emitted Z is below the stock bottom minus the operation's
  explicit through-cut allowance (default 0). Checked on every program in
  tests by parsing the emitted G-code, not by trusting the generator.
- **S2** Rapid moves occur only at or above safe Z.
- **S3** Every cutting and plunging move has an explicit, user-supplied feed.
  Export fails if any tool lacks feed, plunge, or RPM.
- **S4** Spindle is started with the tool's RPM before the first cutting move
  and stopped in the footer.
- **S5** The program begins with the post-processor's units command and
  work-coordinate select, so a controller in the wrong state is corrected.
- **S6** Machine origin is applied as a translation and axis mapping only.
  No mirroring occurs in export (ADR-0001).

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
