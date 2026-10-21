- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_lead`) compares
  the lap and the lead arcs within 0.01 mm plus rounding. The ramp that
  reaches the staging point is checked, not compared, as in `profile-ramp`;
  the lead arcs' feed is checked, not compared.
- DreamCarve project: `tests/golden/profile-lead/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; a 100 × 50 mm rectangle at (25, 25), outside Profile 4 mm deep in
  one pass, climb, 15° ramp entry, `lead: { radius_mm: 6.35, sweep_deg:
  90, lead_out: true, overcut_mm: 0 }`. In the `.camj`: an `add`
  rectangle from Z 12 to 8, `edge_route_outside`, `xyLeadStrategy: arc`,
  `entryStrategy: ramp`.
- PureCutCNC leads only with a ramped or helical entry (`xyLeadNeedsRampedEntry`
  otherwise), so both projects ramp. PureCutCNC's largest lead is one
  tool diameter at 90°, the top of DreamCarve's ladder for this radius
  (#180, #185).
- The DreamCarve rectangle is drawn from its top-right corner. The lead's
  arrival ties on the order of the contour's edges (#185), and drawn this
  way DreamCarve's contour starts where PureCutCNC's does, so both leads
  arrive at the same point: a quarter of the way down the right side.
- One pass: DreamCarve plans one lead per path and comes down at the same
  staging point every pass (#185). PureCutCNC plans one per level and
  stays down between them.
- **Intentional differences.** The ramp is placed differently, as in
  `profile-ramp` (#165). DreamCarve cuts its lead arcs at the cutting
  feed (#185); PureCutCNC holds one reduced feed across a lead, here the
  plunge rate. The test checks DreamCarve's at the tool's feed and
  PureCutCNC's below it (`Difference::LeadArcsAtCuttingFeed`).
