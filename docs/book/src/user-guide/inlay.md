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
  "name": "DC",
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
| `outline_rounding_mm` | `null`, or how much the male's band is rounded, above 0: its outer edge curves in no tighter than this radius, and gaps narrower than about twice it fill. The female ignores it. See [The band's outer edge](#the-bands-outer-edge). |
| `clearing` | End-mill clearing passes for the flat floor, as a [V-Carve's](v-carve.md#clearing-passes), largest end mill first. `[]` for none. On a male, the first whose end mill fits the band's floor also cuts the band's outer edge upright. |
| `step_over_fraction` | How far apart the V-bit's rings are where it clears floor itself, as a fraction of its cut width at the floor, above 0 and at most 1. `null` uses the tool's. Always needed: both sides have a floor. Use 0.1. See [The V-bit floor](v-carve.md#the-v-bit-floor). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. An Inlay is cut in one pass, so both give the same program. |
| `through_cut_allowance_mm` | As every operation has. 0 for an inlay. |
| `safe_z_mm` | The height above the stock top the bit travels at between cuts. |
| `cut_out` | Male only: `null`, or an end-mill pass that frees the plug from its board, run last: `{ "tool": …, "tabs": …, "through_cut_allowance_mm": … }`. See [Cutting the plug free](#cutting-the-plug-free). `null` on a female. |

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

Draw the design as it should read in the finished board, closed outlines
only, and never mirror it yourself; [Preparing your own
design](../quick-start/inlay.md#preparing-your-own-design) lists what to
check in your drawing program. Make both projects from the same drawing,
with `-o` to name each:

```text
dreamcarve init dc-inlay.svg -o dc-female.dcv
dreamcarve init dc-inlay.svg -o dc-male.dcv
```

Then add the same tools to both, paste the same Inlay into both, and
change `side` to `"male"` in the second, and give it a
[`cut_out`](#cutting-the-plug-free) if the machine is to cut the plug
free; the female's stays `null`. Set each project's
`thickness_mm` to its own board's measured thickness. Each exports its
own programs, named after its file: `dc-female-1-T2.nc` and so on.

## The female

The female is a [V-Carve](v-carve.md) of the design with a flat depth of
`depth_mm`, starting at the stock top, move for move: the walls slope
from the outline at the surface down to a floor `depth_mm` deep, and the
V-bit runs up into every corner, so each comes out sharp at the surface.
Shapes nested in the design stand in the pocket as islands, as the
quick start's D's counter does.

## The male

The male cuts the **surround**: the design mirrored, then offset outward
by `margin_mm` and rounded (see [The band's outer
edge](#the-bands-outer-edge)), less the mirrored design itself. So the band round the
design is carved, and so is every hole in the design: the quick start's
D's counter is a hole through the plug, which the counter left standing
in the pocket fills. The design stands as the plug.

- **Start plane.** The male's walls pass exactly through the mirrored
  outline at its start plane, `depth_mm − glue_gap_mm` below the top.
  Above it, the plug narrows as the pocket narrows going down, so the two
  walls meet along their whole height when the plug is turned over.
- **Floor.** The band is cut flat `plug_depth_mm` below the start plane,
  from the plug's foot out to the band's outer edge.
- **The plug base.** Under the band, the board holds the plug together
  until the glue dries (Corbin's "Plug Base Thickness"). Less than 6 mm
  of it, `thickness_mm − (depth_mm − glue_gap_mm + plug_depth_mm)`, warns
  `inlay_plug_base_thin`: use a thicker board for the male.

`check` and `export` say what each side cuts:

```text
Inlay op1 `DC` (female): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm where no clearing pass reaches.
Inlay op1 `DC` (male): deepest cut 5.000 mm below the stock top, the plug's start plane 3.000 mm below the stock top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm where no clearing pass reaches; clearing pass `t2` cuts the band's outer edge upright, its inward curves rounded to 10.000 mm.
```

### The band's outer edge

Only the plug's walls have to meet the pocket's. Everything beyond the
cut-out, or beyond your saw cut, is waste. So the band's outer edge is
cut the cheapest way the tools allow:

- **With a clearing pass that fits**, the end mill cuts it: an upright
  wall from the top of the board down to the band's floor. The pass that
  does it, the **edge pass**, is the first in `clearing` whose end mill
  fits the band's floor, which runs from the plug's foot, `plug_depth_mm
  × tan(α/2)` out from the design, to the band's edge, `margin_mm` out:
  `margin_mm − plug_depth_mm × tan(α/2)` must be at least its diameter.
  The V-bit then cuts only the plug's walls, the narrow places between
  shapes, and the corners of the floor the end mills cannot reach, and
  never comes near the edge. Beyond the edge the board stands to its full
  height, so the safety rules hold the V-bit's whole cone clear of it,
  and the margin must also leave room for the bit's flank at the plug's
  foot: `margin_mm` at least `(depth_mm − glue_gap_mm + 2 ×
  plug_depth_mm) × tan(α/2)`, 4.04 mm with the quick start's values.
- **Without one**, the V-bit cuts the edge as it cuts the plug's walls:
  a wall sloping out at the bit's angle, its foot `plug_depth_mm ×
  tan(α/2)` inside the band at the floor. When the Inlay has clearing
  passes but none fits, `check` warns `inlay_band_edge_v_wall` and names
  the margin that would let one.

**Rounding.** Grown by the margin alone, the band's edge copies every
notch of the design: the dip between two letters, the pinch where they
meet, the inside of a C. `outline_rounding_mm` smooths it. The band is
the design grown by `margin_mm` and **closed** by that radius: grown by
`margin_mm + outline_rounding_mm`, then shrunk by `outline_rounding_mm`.
Where the edge would curve in tighter than the radius it now runs on an
arc of that radius; gaps and notches narrower than about twice the
radius fill in; and nowhere does the band reach farther out than it did,
so it never runs off the board where it did not before. Its outward
curves keep their shape. With the quick start's 10 mm, the edge bridges
the dips over and under the gap between the letters in smooth curves and
fills the C's mouth, so the end mill sweeps round the letters in a few
long, even curves.

With the edge open, the band is rounded by at least the edge pass's
radius even with `outline_rounding_mm: null`, so the end mill reaches
every inward corner of it. A larger rounding costs a little end-mill
time, not fit: only the waste grows. The female ignores the value; set it
the same in both projects all the same, as `margin_mm`.

### The mirror, and which way to flip the plug

The plug is glued in face down, so the male cuts the design **mirrored
left to right**, about the vertical line through the middle of its
targets' bounding box. `check` and `export` say so once for each male
Inlay, with the line's X:

```text
warning: inlay_male_mirrored: operation `op1`: it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
```

Draw the design as it should read in the finished board, and never
mirror it yourself. The program itself is not flipped: its moves are the
mirrored toolpath, in the project's coordinates.

So the quick start's "DC" is carved on the male as a reversed "DC", the
C on the left and the D on the right, both facing left. To fit, turn the
plug over **left to right**, about a vertical line, like turning a page:
over its left or right edge, so that its top edge (+Y on the machine)
stays at the top. Face down, it reads "DC" again. Turned over top to
bottom instead, over its top or bottom edge, an asymmetric design ends
up rotated half a turn and does not fit until you spin it round. Pencil
"top" on each board's back edge before you cut it.

### Cutting the plug free

Outside the band the male board is still at full thickness, and it would
rest on the female's surface before the plug could enter. The plug must
come out of its board before the test fit. The male's `cut_out` does
that with an end mill, as Corbin's add-in's optional "Plug Cutout"
contour does:

```json
"cut_out": {
  "tool": "t2",
  "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true },
  "through_cut_allowance_mm": 0.2
}
```

| Field | Meaning |
| --- | --- |
| `tool` | A flat end mill, long enough to go through the board (see **Flute length** below). Its feed, plunge rate, speed and `step_down_mm` are used; each pass goes one step-down deeper. |
| `tabs` | Holding tabs, as a [Profile's tab request](tabs.md): how many or where, their width along the cut, their top above the bottom of the board, and their shape. `null` cuts the plug free on the last pass: hold it down another way, with double-sided tape under it. |
| `through_cut_allowance_mm` | How far below the board the last pass goes, into the spoilboard: 0.2 mm is usual. |

The cut-out is the operation's last tool run, after the V-bit, so the
plug is freed only once it is carved; with the same end mill as the
clearing pass the tool is fitted twice, and `check` says
`tool_change_avoidable`, which you can ignore: the order is required.
Its program is named `… cut-out t2`.

**Where it cuts.** The band's floor is flat between the plug's foot and
the band's outer edge: at the floor, the V-bit's flank stands
`plug_depth_mm × tan(α/2)` off the design on the plug's side. With an
open edge the floor runs out to the edge itself, and the end mill's
centre runs in its middle, `(margin_mm + plug_depth_mm × tan(α/2)) / 2`
out from the mirrored design's outer outlines: 4.577 mm in the quick
start. With a V-walled edge the floor stops `plug_depth_mm × tan(α/2)`
inside the edge too, and the centre runs half `margin_mm` out. Either
way it runs round each convex corner on an arc, and is rounded as the
band is: grown by that distance plus `outline_rounding_mm`, then shrunk
by `outline_rounding_mm`. It never runs at the band's outer edge: a
V wall there would leave a sharp edge, the full height of the board,
standing round the freed piece, and it would land on the female's
surface. The piece
it frees is the plug on its backing, with a rim of the band's floor
round it, `plug_depth_mm` below the plug's start plane, which clears the
female's surface by that much when the plug is seated. A hole in the
design (the D's counter in the quick start) is never cut round: it stays
in the plug.

**Room.** The floor ring must be at least the end mill's diameter wide;
else `check` refuses it, `inlay_cut_out_no_room`, and names the margin
needed. With an open edge it is `margin_mm − plug_depth_mm × tan(α/2)`
wide; with a V wall, `margin_mm − 2 × plug_depth_mm × tan(α/2)`. With the
60° bit and a 2 mm plug depth, a 6.35 mm cut-out needs a margin of at
least 7.505 mm when a clearing pass cuts the edge, and 8.66 mm (use 10)
without one; a 3.175 mm cut-out needs 4.33 mm and 5.48 mm.

**Flute length.** The cut-out goes through the board, so the end mill's
flutes must be at least the board's thickness plus
`through_cut_allowance_mm` plus 2 mm long: 14.2 mm for a 12 mm board
with 0.2 mm through. DreamCarve does not know a tool's flute length and
cannot check it. A 1/8 in end mill often has only 12.7 mm (1/2 in); for a
thick male board use one with longer flutes, a 1/4 in end mill with a
wider margin, or `"cut_out": null` and a saw.

**Depth.** The passes start at the band's floor, `depth_mm −
glue_gap_mm + plug_depth_mm` below the top, so none cuts the air the
carve has removed: the first lap is one step-down below the floor, and
each goes a step-down deeper, through the board to
`through_cut_allowance_mm` below it. The end mill gets there from the
stock top plus 1 mm at its plunge rate. The tabs' heights count from the
bottom of the board, as a Profile's do, so they must be below the floor:
on a 12 mm board with a 5 mm deep floor, below 7 mm. Each pass below a
tab's top lifts over it.

After the cut-out, cut the tabs with a knife or a fine saw and lift the
plug out. Without a cut-out (`"cut_out": null`), saw round the plug,
through the board, inside the band, between the plug's foot and the
band's outer edge: a bandsaw or scroll saw.

## Clearing passes

Either side takes [clearing passes](v-carve.md#clearing-passes), as a
V-Carve does: each end mill clears the floor (the female's pocket floor at
`depth_mm`, or the male's band and holes at `depth_mm − glue_gap_mm +
plug_depth_mm`) as a Pocket, in a tool run, and a program, of its own,
before the V-bit. The V-bit then traces the floor's edge along the
walls, so the floor and the walls meet cleanly, and clears only what no
end mill reached: the corners. A clearing pass makes a large floor much
faster, and flatter. On the male, the first pass that fits the band's
floor clears it out to the band's outer edge, which it cuts upright (see
[The band's outer edge](#the-bands-outer-edge)).

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
"DC", the serifs' tips and the C's pointed terminals end, 3 mm down,
well short of where they are drawn. The plug's points are the same shape
turned over, so the two still meet; but the last few millimetres of each
point are only a shallow V. That is expected: the points are sharp at the surface, where
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
the flank engaged from the stock top. When a clearing pass cuts the male's
band edge upright, the board beyond that edge stands to the stock top, so
the V-bit's cone is also held inside the band right up to the stock top.
The band, its rounding and which pass cuts its edge are worked out again
from the project. A clearing pass is held as a Pocket of its floor. A
male's cut-out is held to the band's flat floor ring,
worked out again from the project: the end mill's whole cutter stays
clear of both its edges at every depth, it goes no deeper than its own
`through_cut_allowance_mm` below the board, and it never plunges into one
of its tabs.

## Errors

These stop `check` and `export`.

| Code | Message says | What to do |
| --- | --- | --- |
| `operation_value_unset` | "operations[…].depth_mm is not set", and the same for `glue_gap_mm`, `plug_depth_mm`, and on a male `margin_mm` | Set the value. |
| `out_of_range` | `depth_mm` or `plug_depth_mm` is not above 0, or `glue_gap_mm` is below 0 | Set a value in range. |
| `inlay_glue_gap_not_below_depth` | "… has glue_gap_mm … mm, not below its depth_mm … mm; the male starts depth_mm − glue_gap_mm below the stock top, so its plug would have no wall to seat …" | Make the glue gap smaller than the depth. |
| `inlay_margin_positive` | "… is a male inlay with margin_mm … mm; its surround is carved that far beyond the design, so it must be above 0 …" | Set a margin above 0; 5 to 8 mm. |
| `inlay_outline_rounding` | "… has outline_rounding_mm … mm; the male's band is rounded by that radius, so it must be above 0 …" | Set a radius above 0, or `null` for none. |
| `v_carve_needs_v_bit` | "operation … carves with tool …, an end mill; a carve's depth follows the angle of a V-bit, so use a V-bit" | Give the Inlay a V-bit. |
| `v_bit_angle_range` | "tool … has an included angle of … degrees; …" | Fix the tool's `included_angle_deg`. |
| `v_bit_cone_exceeded` | "operation … engages tool … … mm deep …"; on a male "… from the stock top (its start plane, depth_mm − glue_gap_mm below the stock top, plus its plug_depth_mm; the plug above the start plane is never cleared)" | See [Choosing the bit](#choosing-the-bit). On a female it is at `depth_mm`, on a male at `plug_depth_mm`. |
| `inlay_cut_out_needs_male` | "… is a female inlay with a cut_out; only the male's plug is cut out of its board …" | Set the female's `cut_out` to `null`. |
| `inlay_cut_out_no_room` | "… cuts its plug out with tool …, … mm across, in the middle of the band's flat floor; with margin_mm … mm that floor is narrower than the tool … Raise margin_mm to at least … mm in both projects, or use a narrower end mill" | Raise `margin_mm` in both projects, or use a narrower end mill. |
| `cutter_not_flat` | "… cuts its plug's cut-out with tool …, a V-bit; …" at `cut_out.tool` | Give the cut-out a flat end mill. |

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
| `inlay_band_edge_v_wall` | A male has clearing passes, but none fits its band's floor, so the V-bit cuts the band's outer edge as a sloped wall; it names the margin that would let a pass cut it upright. |
| `vcarve_target_open` | A target is an open path; it is skipped. |
| `clearing_no_room`, `clearing_not_descending` | As for a [V-Carve](v-carve.md#warnings). |

[Warnings](warnings.md) says what to do about each.

## Limits

- 2D inlays on flat boards only; no inlays on curved surfaces.
- Two projects, not one: DreamCarve cannot check that both use the same
  drawing, values and bit angle. Change them together.
- The cut-out frees the plug on its backing; the backing is not removed:
  that is a saw and a plane, after the glue-up.
- The cut-out has no entry ramp or lead: it plunges at the plunge rate,
  a step-down at a time.
- The bit is an ideal point; a real tip has a small flat. The fit
  procedure absorbs it, as it absorbs a bit whose angle is a little off.
