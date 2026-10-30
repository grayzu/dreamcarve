# Inlay operation

An **Inlay** cuts one side of a V-bit inlay: a design in one wood set
flush into another. It takes two boards and two projects made from the
same drawing:

- the **female** side cuts the design as a **pocket** in one board, with
  walls that slope at the V-bit's angle;
- the **male** side cuts the design, mirrored, as a **plug** standing on
  another board, with walls that slope the other way.

Turned over, the plug fits the pocket: its walls meet the pocket's walls
along their whole height, and its face stops a little short of the
pocket's floor, leaving room for glue. You glue it in, saw off the plug's
board above the surface, and plane flat. Because both sides are cut with
the same V-bit, every corner of the design can be sharp, which no
end-mill inlay can do.

The method, its four values and the fit procedure follow Corbin's
Workshop's V-bit inlay documentation for Fusion
([terminology](https://docs.corbinsworkshop.com/terminology.md),
[the inlay feature](https://docs.corbinsworkshop.com/vbit_inlay_feature_tab.md),
[plug setup](https://docs.corbinsworkshop.com/plug-setup-2d.md),
[fit adjustment](https://docs.corbinsworkshop.com/inlay_fit_adjustment.md)).
This chapter gives each value Corbin's name next to its own.

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Inlay, the female of the [inlay pair quick
start](../quick-start/inlay.md):

```json
{
  "kind": "inlay",
  "id": "op1",
  "name": "Star",
  "enabled": true,
  "targets": [ "i1", "i2" ],
  "tool": "t3",
  "side": "female",
  "depth_mm": 4.0,
  "glue_gap_mm": 1.0,
  "plug_depth_mm": 2.0,
  "margin_mm": 8.0,
  "clearing": [
    { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
  ],
  "step_over_fraction": 0.1,
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 10.0
}
```

The male is the same with `"side": "male"`.

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances that make the design. Every closed outline of each takes part, nested as for a [V-Carve](v-carve.md#medial-lines-and-depth): an outline inside another is a hole in the design. Open paths are skipped with the warning `vcarve_target_open`. |
| `tool` | The id of the tool. It must be a V-bit, the same angle on both sides. See [Choosing the bit](#choosing-the-bit). |
| `side` | `"female"`, the pocket, or `"male"`, the plug. The only field that differs between the two projects. |
| `depth_mm` | How deep the female's pocket is, above 0. See [The four values](#the-four-values). |
| `glue_gap_mm` | The gap left under the seated plug, 0 or more and below `depth_mm`. The value you change to adjust the fit. |
| `plug_depth_mm` | How much deeper than its walls need the male cuts the band round the plug, above 0. |
| `margin_mm` | How far beyond the design the male's band reaches, above 0. The female ignores it; it may be `null` there. |
| `clearing` | End-mill clearing passes for the flat floor, as a [V-Carve's](v-carve.md#clearing-passes), largest end mill first. `[]` for none. |
| `step_over_fraction` | How far apart the V-bit's rings are where it clears floor itself, as a fraction of its cut width at the floor, above 0 and at most 1. `null` uses the tool's. Always needed: both sides have a floor. Use 0.1. See [The V-bit floor](v-carve.md#the-v-bit-floor). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. An Inlay is cut in one pass, so both give the same program. |
| `through_cut_allowance_mm` | As every operation has. 0 for an inlay. |
| `safe_z_mm` | The height above the stock top the bit travels at between cuts. |

The depth, glue gap and plug depth are needed on either side, and the
margin on a male, so that one operation, copied, describes both: `null`
until set, and `check` names each one still unset.

## The four values

All four are the same in both projects. Change one, and change it in
the other.

| Field | Corbin's name | What it sets | What changing it does |
| --- | --- | --- | --- |
| `depth_mm` (D) | Inlay Depth | The female's pocket depth: its walls slope from the surface down to a flat floor D deep. | A deeper inlay survives more planing. Corbin uses about 2.5 mm for a decorative inlay and about 6 mm for a cutting board. The male's plug grows with it. |
| `glue_gap_mm` (G) | Inlay Fit Adjustment | The gap between the seated plug's face and the pocket's floor. The male's **start plane**, where its walls pass exactly through the design's outline, is `D − G` below its top, so the plug is `D − G` tall. | Larger: a shorter plug, that seats sooner on its walls, with more gap under it. Smaller: a taller plug, with less gap under it; 0 makes the plug exactly as tall as the pocket is deep. See [The fit](#the-fit). Corbin starts at 0 or 0.25 mm. |
| `plug_depth_mm` (P) | Plug Depth (Corbin's surface gap plus his fit adjustment) | How far below the start plane the male's band is cut flat. The band's floor is `D − G + P` below the male's top. | With the plug seated, the band, and the rest of the male board, stand P above the female's surface: the room you saw through to take the backing off. Corbin's surface gap is 1.3 to 1.8 mm. |
| `margin_mm` (M) | Plug Stock Offset | How far beyond the design the male's band reaches, at the start plane. | The band you cut the plug free in. Corbin uses 5 to 7.5 mm; 8 mm is easy to saw in. Larger costs time, not fit. |

With the quick start's D 4, G 1, P 2: the pocket is 4 mm deep; the plug is
3 mm tall above its start plane, 3 mm below the male's top; the band's
floor is 5 mm below the top; seated, the plug's face is 1 mm above the
pocket's floor and the band 2 mm above the female's surface.

## Choosing the bit

A V-bit cuts only with its cone; below the **cone height**, `diameter /
(2 × tan(α / 2))` for angle `α`, its straight shank would cut. An Inlay's
flank is engaged from the stock top on both sides:

- the **female** engages it `depth_mm` deep;
- the **male** engages it `depth_mm − glue_gap_mm + plug_depth_mm` deep:
  the plug's own wood stands above the start plane, and it is never
  cleared, so the bit's flank cuts from the board's top down to the band's
  floor.

Either past the cone height is the error `v_bit_cone_exceeded`. For the
common 1/4 in (6.35 mm) bits:

| Angle | Cone height | The quick start's 4, 1, 2 (5 mm on the male) | Corbin's usual 2.5, 0.25, 1.5 (3.75 mm on the male) |
| --- | --- | --- | --- |
| 15° | 24.1 mm | fits | fits |
| 30° | 11.8 mm | fits | fits |
| 60° | 5.5 mm | fits | fits |
| 90° | 3.175 mm | refused: the female's 4 mm too | refused on the male |

With the 90° 1/4 in bit, Corbin's usual values are refused on the male:
2.5 − 0.25 + 1.5 = 3.75 mm, past the 3.175 mm cone, though the female's
2.5 mm fits. Lower the values so that the sum fits, for example
`depth_mm` 2, `glue_gap_mm` 0.25 and `plug_depth_mm` 1.25 (3 mm), or use a
60° bit, or a 90° bit 12.7 mm (1/2 in) across, whose cone is 6.35 mm.

A steeper bit (a smaller angle) makes deeper walls for the same width,
which glue better and survive more planing, and it reaches farther into
narrow parts of a design. A 60° bit is a good start. Both projects must
use the same angle; two files cannot check that for you.

## Two projects from one drawing

Make both projects from the same drawing, with `-o` to name each:

```text
dreamcarve init star.svg -o star-female.dcv
dreamcarve init star.svg -o star-male.dcv
```

Then add the same tools to both, paste the same Inlay into both, and
change `side` to `"male"` in the second. Set each project's
`thickness_mm` to its own board's measured thickness. Each exports its
own programs, named after its file: `star-female-1-T2.nc` and so on.

## The female

The female is a [V-Carve](v-carve.md) of the design with a flat depth of
`depth_mm`, starting at the stock top, move for move: the walls slope
from the outline at the surface down to a floor `depth_mm` deep, and the
V-bit runs up into every corner, so each comes out sharp at the surface.
Shapes nested in the design stand in the pocket as islands, as the
quick start's circle does.

## The male

The male cuts the **surround**: the design mirrored, then offset outward
by `margin_mm`, less the mirrored design itself. So the band round the
design is carved, and so is every hole in the design: the quick start's
circle is a hole through the plug, which its standing walnut circle
fills. The design stands as the plug.

- **Start plane.** The male's walls pass exactly through the mirrored
  outline at its start plane, `depth_mm − glue_gap_mm` below the top.
  Above it, the plug narrows as the pocket narrows going down, so the two
  walls meet along their whole height when the plug is turned over.
- **Floor.** The band is cut flat `plug_depth_mm` below the start plane.
  The band's outer edge gets a sloped wall too.
- **The plug base.** Under the band, the board holds the plug together
  until the glue dries (Corbin's "Plug Base Thickness"). Less than 6 mm
  of it, `thickness_mm − (depth_mm − glue_gap_mm + plug_depth_mm)`, warns
  `inlay_plug_base_thin`: use a thicker board for the male.

`check` and `export` say what each side cuts:

```text
Inlay op1 `Star` (female): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm where no clearing pass reaches.
Inlay op1 `Star` (male): deepest cut 5.000 mm below the stock top, the plug's start plane 3.000 mm below the stock top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm where no clearing pass reaches.
```

### The mirror, and which way to flip the plug

The plug is glued in face down, so the male cuts the design **mirrored
left to right**, about the vertical line through the middle of its
targets' bounding box. `check` and `export` say so once for each male
Inlay, with the line's X:

```text
warning: inlay_male_mirrored: operation `op1`: it is a male inlay, so its design is cut mirrored about X = 50 mm, the middle of its targets, to seat face down in the female
```

Draw the design as it should read in the finished board, and never
mirror it yourself. The program itself is not flipped: its moves are the
mirrored toolpath, in the project's coordinates.

To fit, turn the plug over **left to right**, about a vertical line,
like turning a page, so that its top edge (+Y on the machine) stays at
the top. Turned over top to bottom instead, an asymmetric design ends up
rotated half a turn and does not fit. Pencil "top" on each board's back
edge before you cut it.

### Cutting the plug free

Outside the band the male board is still at full thickness, and it would
rest on the female's surface before the plug could enter. Before the test
fit, saw round the plug, through the board, inside the band, between the
plug's foot and the band's outer wall: a bandsaw or scroll saw, or a
Profile of your own on an outline drawn inside the band, as Corbin cuts
the plug's silhouette out with a contour toolpath.

## Clearing passes

Either side takes [clearing passes](v-carve.md#clearing-passes), as a
V-Carve does: each end mill clears the floor (the female's pocket floor at
`depth_mm`, or the male's band and holes at `depth_mm − glue_gap_mm +
plug_depth_mm`) as a Pocket, in a tool run, and a program, of its own,
before the V-bit. The V-bit then traces the floor's edge, so the floor
and the walls meet cleanly, and clears only what no end mill reached: the
corners. A clearing pass makes a large floor much faster, and flatter.

Without clearing passes, the V-bit clears the floor itself, in rings
`step_over_fraction` of its cut width apart, which leave ridges
`step_over_fraction` × the floor's depth below the start plane high.
`check` prints the ridge height. Neither floor shows in the finished
board: the female's is under the glue gap, the male's is sawn off. But
ridges under the plug can hold it up, so keep `step_over_fraction` at 0.1
or clear the floor with an end mill.

## The fit

Fit is found by testing, as Corbin's fit adjustment procedure does:
cut the female, cut the male, cut the plug free, and press it into the
pocket dry, without glue.

- **It seats with no gap at the surface**, and its backing stops short
  of the female's surface: the fit is right. Glue it.
- **A gap at the surface, all round**, the plug in as far as it goes:
  the plug's face reached the pocket's floor before its walls met the
  pocket's. Raise `glue_gap_mm`, by 0.25 mm at a time, in both projects,
  and cut a new male. A larger gap makes a shorter plug that seats on its
  walls first.
- **The plug will not go in**, or stops well short: lower `glue_gap_mm`,
  by 0.25 mm at a time, and cut a new male. The plug gets taller, its
  start plane deeper in the male board, which is Corbin's remedy for a
  plug that is not deep enough to seat.
- **The top looks right, but a test pair cut in half shows a gap under
  the plug** bigger than you want: lower `glue_gap_mm`; the walls still
  meet, and the glue line under the plug is thinner.

Only the male changes: the female's pocket depends on `depth_mm` alone.
Once you have found the value for a bit, a pair of woods and your
machine, keep it. A fit that needs a glue gap close to `depth_mm`
points at the bit: its real angle or tip is not what the tool says.

A press, or a clamp with a flat caul on each side, closes the fit
further: hardwood compresses a little as the walls are forced together.

### Corners and points

Both sides carve every corner to a point: a V-bit cannot cut a sharp
corner below the surface, so at each depth the pocket is the outline
shrunk by the wall's slope, and a sharp point shrinks back along its
length much more than a straight side moves in. In the quick start's
star, 3 mm down the pocket's points end several millimetres short of the
drawn tips. The plug's points are the same shape turned over, so the two
still meet; but the last few millimetres of each point are only a
shallow V. That is expected: the points are sharp at the surface, where
they show, and they are the part to treat gently when you saw and plane.
Parts of a design narrower than the bit can reach into come out as fine
V's on both sides.

## Gluing and planing

1. Spread glue on the plug's walls and in the pocket. The gap under the
   plug takes the excess.
2. Turn the plug over left to right, seat it, and press it home with a
   clamp or a press and flat cauls, until glue squeezes out all round.
   Leave it to dry as the glue says.
3. Saw off the backing through the gap between the male's band and the
   female's surface, `plug_depth_mm` wide.
4. Plane or sand the board flat, down to the female's surface.

## Safety

Every Inlay program passes the [safety](safety.md) rules, including rule
14 (S17), which holds the V-bit's cone to its side's own material: the
design for the female; the surround (the band and the design's holes) for
the male, with the start plane `depth_mm − glue_gap_mm` below the top and
the flank engaged from the stock top. A clearing pass is held as a Pocket
of its floor.

## Errors

These stop `check` and `export`.

| Code | Message says | What to do |
| --- | --- | --- |
| `operation_value_unset` | "operations[…].depth_mm is not set", and the same for `glue_gap_mm`, `plug_depth_mm`, and on a male `margin_mm` | Set the value. |
| `out_of_range` | `depth_mm` or `plug_depth_mm` is not above 0, or `glue_gap_mm` is below 0 | Set a value in range. |
| `inlay_glue_gap_not_below_depth` | "… has glue_gap_mm … mm, not below its depth_mm … mm; the male starts depth_mm − glue_gap_mm below the stock top, so its plug would have no wall to seat …" | Make the glue gap smaller than the depth. |
| `inlay_margin_positive` | "… is a male inlay with margin_mm … mm; its surround is carved that far beyond the design, so it must be above 0 …" | Set a margin above 0; 5 to 8 mm. |
| `v_carve_needs_v_bit` | "operation … carves with tool …, an end mill; a carve's depth follows the angle of a V-bit, so use a V-bit" | Give the Inlay a V-bit. |
| `v_bit_angle_range` | "tool … has an included angle of … degrees; …" | Fix the tool's `included_angle_deg`. |
| `v_bit_cone_exceeded` | "operation … engages tool … … mm deep …"; on a male "… from the stock top (its start plane, depth_mm − glue_gap_mm below the stock top, plus its plug_depth_mm; the plug above the start plane is never cleared)" | See [Choosing the bit](#choosing-the-bit). On a female it is at `depth_mm`, on a male at `plug_depth_mm`. |

The clearing errors (`clearing_needs_end_mill`, `clearing_dangling_tool`)
are as a [V-Carve's](v-carve.md#errors). An Inlay has no start depth, so
the start depth errors never name one, and it always has a floor, so
`clearing_needs_flat_depth` never does either. Export also stops when
neither the operation nor its V-bit sets `step_over_fraction`.

## Warnings

| Code | When |
| --- | --- |
| `inlay_male_mirrored` | Every male Inlay, once: a note giving the mirror line. |
| `inlay_plug_base_thin` | A male leaves less than 6 mm of board under its band. |
| `vcarve_target_open` | A target is an open path; it is skipped. |
| `clearing_no_room`, `clearing_not_descending` | As for a [V-Carve](v-carve.md#warnings). |

[Warnings](warnings.md) says what to do about each.

## Limits

- 2D inlays on flat boards only; no inlays on curved surfaces.
- Two projects, not one: DreamCarve cannot check that both use the same
  drawing, values and bit angle. Change them together.
- The plug is not cut out of its board, and the backing is not removed:
  that is a saw, or a Profile you draw.
- The bit is an ideal point; a real tip has a small flat. The fit
  procedure absorbs it, as it absorbs a bit whose angle is a little off.
