# Inlay operation

An **Inlay** cuts a V-bit inlay: a design in one wood set flush into
another. One operation, in one project, cuts both boards:

- the **female** cuts the design as a **pocket** in the project's
  stock, with walls that slope at the V-bit's angle;
- the **male** cuts the design, mirrored, as a **plug** standing on the
  Inlay's **male board**, a second board beside the stock, with walls
  that slope the other way.

Turned over, the plug fits the pocket: its walls meet the pocket's walls
along their whole height, and its face stops a little short of the
pocket's floor, leaving room for glue. You glue it in, saw off the plug's
board above the surface, and plane flat. Because both boards are cut
with the same V-bit, every corner of the design can be sharp, which no
end-mill inlay can do.

The method, its four values and the fit procedure follow Corbin's
Workshop's V-bit inlay documentation for Fusion
([terminology](https://docs.corbinsworkshop.com/terminology.md),
[the inlay feature](https://docs.corbinsworkshop.com/vbit_inlay_feature_tab.md),
[plug setup](https://docs.corbinsworkshop.com/plug-setup-2d.md),
[fit adjustment](https://docs.corbinsworkshop.com/inlay_fit_adjustment.md)).
This chapter gives each value Corbin's name next to its own.

Add it to the project's `operations` list by hand; `init` writes only a
Profile. A complete Inlay, the one of the [inlay pair quick
start](../quick-start/inlay.md):

```json
{
  "kind": "inlay",
  "id": "op1",
  "name": "DC",
  "enabled": true,
  "targets": [ "i1", "i2" ],
  "tool": "t3",
  "male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 12.0 },
  "depth_mm": 4.0,
  "glue_gap_mm": 1.0,
  "plug_depth_mm": 2.0,
  "margin_mm": 8.0,
  "outline_rounding_mm": 10.0,
  "clearing": [
    { "tool": "t2", "strategy": "offset", "direction": "climb", "entry": { "ramp": { "angle_deg": 15.0 } } }
  ],
  "step_over_fraction": 0.1,
  "machining_order": "full_depth_first",
  "through_cut_allowance_mm": 0.0,
  "safe_z_mm": 10.0,
  "cut_out": { "tool": "t2", "tabs": { "placement": { "kind": "count", "count": 4 }, "width_mm": 6.0, "height_mm": 3.0, "shape": "rect", "on_holes": false, "avoid_corners": true }, "through_cut_allowance_mm": 0.2 }
}
```

The project's stock is the female board. `male_board` is the male's: the
same width and height as the stock (`null`), 12 mm thick. See [One
project, two boards](#one-project-two-boards).

## Settings

| Field | Meaning |
| --- | --- |
| `id` | The operation's id, shown in messages. |
| `name` | Your name for it, written in the program. |
| `enabled` | `false` leaves the operation out of the program, with a warning. |
| `targets` | The ids of the instances that make the design. Every closed outline of each takes part, nested as for a [V-Carve](v-carve.md#medial-lines-and-depth): an outline inside another is a hole in the design. Open paths are skipped with the warning `vcarve_target_open`. |
| `tool` | The id of the tool. It must be a V-bit; it cuts both boards. See [Choosing the bit](#choosing-the-bit). |
| `male_board` | The board the male is cut from: `{ "width_mm": …, "height_mm": …, "thickness_mm": … }`. Width and height `null` are the stock's; set them, above 0, for a male board of another size. The thickness is the male board's own, above 0: until it is set, only the female is cut, with the warning `inlay_male_board_unset`. See [One project, two boards](#one-project-two-boards). |
| `depth_mm` | How deep the female's pocket is, above 0. See [The four values](#the-four-values). |
| `glue_gap_mm` | The gap left under the seated plug, 0 or more and below `depth_mm`. The value you change to adjust the fit. |
| `plug_depth_mm` | How much deeper than its walls need the male cuts the band round the plug, above 0. |
| `margin_mm` | How far beyond the design the male's band reaches, above 0. Needed once the male is cut; the female ignores it. |
| `outline_rounding_mm` | `null`, or how much the male's band is rounded, above 0: its outer edge curves in no tighter than this radius, and gaps narrower than about twice it fill. The female ignores it. See [The band's outer edge](#the-bands-outer-edge). |
| `clearing` | End-mill clearing passes for the flat floor, as a [V-Carve's](v-carve.md#clearing-passes), largest end mill first, on both boards. `[]` for none. On the male, the first whose end mill fits the band's floor also cuts the band's outer edge upright. |
| `step_over_fraction` | How far apart the V-bit's rings are where it clears floor itself, as a fraction of its cut width at the floor, above 0 and at most 1. `null` uses the tool's. Always needed: both boards have a floor. Use 0.1. See [The V-bit floor](v-carve.md#the-v-bit-floor). |
| `machining_order` | `"full_depth_first"` or `"pass_first"`. An Inlay is cut in one pass, so both give the same program. |
| `through_cut_allowance_mm` | As every operation has. 0 for an inlay. |
| `safe_z_mm` | The height above each board's top the bit travels at between cuts. |
| `cut_out` | The male's: `null`, or an end-mill pass that frees the plug from the male board, run last: `{ "tool": …, "tabs": …, "through_cut_allowance_mm": … }`. See [Cutting the plug free](#cutting-the-plug-free). The female ignores it. |

The depth, glue gap and plug depth are always needed, and the margin
once the male is cut: `null` until set, and `check` names each one still
unset.

## The four values

One Inlay holds them for both boards, so the female and the male always
agree. Change one, and both boards' programs follow.

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
flank is engaged from each board's top:

- the **female** engages it `depth_mm` deep;
- the **male** engages it `depth_mm − glue_gap_mm + plug_depth_mm` deep:
  the plug's own wood stands above the start plane, and it is never
  cleared, so the bit's flank cuts from the board's top down to the band's
  floor.

Either past the cone height is the error `v_bit_cone_exceeded`; with
the male board's thickness unset, only the female's is checked. For the
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

Both boards are cut with the operation's one V-bit, so their angles
always agree.

### Choosing a V-bit for an inlay

Use a 60° or a 90° bit. With either, the plug seats within about
0.05 mm of the height its walls are cut for. A 60° 1/4 in bit takes the
quick start's values as they are; a 90° 1/4 in bit needs them lowered to
fit its 3.175 mm cone, as above.

A narrower bit, 30° or 15°, makes deeper walls for the same width, which
glue better and survive more planing, and it reaches farther into narrow
parts of a design. But its steep walls turn a small sideways error into
a larger one in height: where the plug's wall sits a little to one side
of the pocket's, the plug seats higher or lower by that error times
1 / tan(α / 2), for angle `α`:

| Angle | Height per unit of sideways error |
| --- | --- |
| 90° | 1 |
| 60° | 1.7 |
| 30° | 3.7 |
| 15° | 7.6 |

**Export narrow-bit inlays in millimetres.** A program has a limit at
very sharp corners, set by its decimal places, and DreamCarve accepts it
rather than working round it. A program writes each point to its decimal
places, so the bit can only start its cut into a corner at a point the
program can write exactly. A millimetre program writes to 0.001 mm and
an inch program to 0.0001 in (0.00254 mm). In a corner a few degrees
wide, the nearest such point can be some way from the tip, and the plug
keeps the wood the bit cannot reach. How sharp a corner must be before
that can hold the plug more than 0.05 mm up depends on the bit and the
places:

| Bit | Millimetres, 3 places | Millimetres, 4 places | Inches, 4 places |
| --- | --- | --- | --- |
| 15° | 11° | 1.2° | 25° |
| 30° | 5.7° | 0.6° | 14° |

The 60° and 90° bits have no such limit: DreamCarve holds them to the
0.05 mm seat at every corner. A corner sharper than the table's angle *can* hold the plug up, not
always: whether it does depends on where the corner's tip falls between
the points the program can write.

- **In inches**, with a 15° or 30° bit, the plug can stand up to about
  0.16 mm proud with the 15° bit and 0.08 mm with the 30° at a 5°
  corner, and more at a point sharper still. Where two overlapping
  letters left a counter ending in a hairline spike, a 30° bit's plug
  stood 0.37 mm proud in inches and seated in millimetres.
- **In millimetres** a 15° bit's plug stood 0.068 mm proud past the tip
  of a 4.5° notch
  ([#559](https://github.com/grayzu/dreamcarve/issues/559)), and seated
  at 0.033 mm with four places.

So for an inlay with a 15° or 30° bit, set the project's `display_units`
to `"mm"` before you export, whatever units you design in. If the design
also has very sharp points, a few degrees wide, set the post-processor's
`decimal_places.mm` to 4 as well
([Post-processors](post-processors.md#settings)):

```json
"decimal_places": {
  "mm": 4,
  "inch": 4
}
```

An inch program already writes four places. The bundled
post-processors keep 3 as their default.

Apart from that limit, corners, sharp or obtuse, seat with every bit,
and so do the other tight features where a narrow bit's plug used to
meet the pocket first: a line that ends inside the design, a curve that
narrows, two parts that nearly touch
([#506](https://github.com/grayzu/dreamcarve/issues/506)). In
millimetres the 30° bit's plugs seat within 0.05 mm. So do the 15°
bit's, as long as the glue gap stands above the floor's ridges at their
corners, as [Clearing passes](#clearing-passes) says: where the V-bit
clears a wide floor itself, in rings, the rings' corners leave peaks
taller than the ridges between them, and on a glue gap set just above
the lower figure a 15° plug rested on one 0.16 mm proud
([#522](https://github.com/grayzu/dreamcarve/issues/522)). Either way,
press the plug home with a clamp and flat cauls, and keep `glue_gap_mm`
at 1 mm or more, so that the plug has room under it to go down.

## One project, two boards

Draw the design as it should read in the finished board, closed outlines
only, and never mirror it yourself; [Preparing your own
design](../quick-start/inlay.md#preparing-your-own-design) lists what to
check in your drawing program. Make one project from the drawing, add
the tools, paste the Inlay, and give it a
[`cut_out`](#cutting-the-plug-free) if the machine is to cut the plug
free.

```text
dreamcarve init dc-inlay.svg -o dc.dcv
```

The project's **stock** is the **female board**: set its `thickness_mm`
to the light board's measured thickness. The stock can carry other
operations too, such as a Pocket or a Profile round the finished piece;
they are cut on the female board with the female's programs.

### The male board

The Inlay's `male_board` is the board the plug is cut from:

```json
"male_board": { "width_mm": null, "height_mm": null, "thickness_mm": 12.0 }
```

- `thickness_mm` is the male board's own measured thickness, never the
  stock's. Until you set it, the male is not cut: `check` and `export`
  give only the female's programs and warn `inlay_male_board_unset`,
  pointing at the value to measure. Set it before you cut the male.
- `width_mm` and `height_mm` are `null` for a male board the same size
  as the stock. Set them for a smaller or larger board. Where the male's
  band runs past the male board's edge it is clipped to the board (see
  **At the board's edge** below).

The male board is a second setup on the machine with the stock's
front-left corner as its own: the design is mirrored in place on it, as
if the stock were the male board. The project's origin preset applies
to it as to the stock: with `bottom_left` work zero is the male board's
front-left corner, with `centre` its centre, and Z is at its top or its
bottom as the preset says. Safe Z, the clearance height and the safety
checks are the male board's too.

### Programs

`export` writes the female board's programs, then the male board's. In
a project with an Inlay every program is named after its board,
`<stem>-female-<k>-T<n>.<ext>` and `<stem>-male-<k>-T<n>.<ext>`, `<k>`
counting from 1 on each board: the quick start's `dc.dcv` writes
`dc-female-1-T2.nc`, `dc-female-2-T1.nc`, `dc-male-1-T2.nc`,
`dc-male-2-T1.nc` and `dc-male-3-T2.nc`. The names keep the board even
when a board has one program, and with a one-program (`M6`)
post-processor, which writes one program per board. A project with no
Inlay names its programs as before.

`export` lists them grouped by board, each with its size, and the first
program of each board says to set work zero (X, Y and Z), since each
board is set up on its own:

```text
Female board, the stock, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-female-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-female-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
Male board, 110.000 mm × 80.000 mm × 12.000 mm:
  1. dc-male-1-T2.nc  Before you start: fit T2 1/8 end mill and set work zero (X, Y and Z) with it.
  2. dc-male-2-T1.nc  Before you start: fit T1 1/4 60 degree V-bit and set Z zero with it. Keep X and Y zero.
  3. dc-male-3-T2.nc  Before you start: fit T2 1/8 end mill and set Z zero with it. Keep X and Y zero.
```

The extents and times are grouped the same way, with an estimated time
for each board that has more than one program, then the total. An
operation line of the male ends in "on the male board", and a warning
about the male names it: ``operation `op1` (male board): …``. `check`
groups its programs under the same headings.

`--preview` draws the male board to the right of the stock, 10 mm
apart, their bottom edges level, each with its dashed outline and its
own origin mark, and the male's mirrored design and toolpaths on it;
the legend marks the male's runs "(male board)".

### Cutting one board again

`--board female` or `--board male` exports one board's programs only.
It is for this export alone and is not saved in the project. After a
fit change, re-cut the male on a new board:

```text
dreamcarve export dc.dcv --board male
```

The female's programs are left as they were. Because one operation
describes both boards, the new male is cut with the same depth, plug
depth, margin and bit as the pocket it must fit. With nothing on the
male board, `--board male` fails: "nothing is cut on the male board: no
enabled Inlay cuts its male (set its male_board.thickness_mm)".

### Two Inlays

Every male of a project is cut on one male board, the first enabled
Inlay's that cuts its male, in operation order. A later Inlay naming a
different `male_board` is the error `inlay_male_boards_differ`: give
them all the same one. (An Inlay whose male board has no thickness cuts
only its female, and is not held to it.)

### Projects from before format 25

Until format 25 an Inlay cut one side, `"side": "female"` or `"male"`,
and a pair was two projects. DreamCarve opens them and upgrades them:

- A **female** project's Inlay gets a male board with nothing set. It
  exports the female programs it did, now named `<stem>-female-<k>-T<n>`,
  and warns `inlay_male_board_unset`.
- A **male** project's Inlay gets a male board the size and thickness of
  its stock. It exports the male programs it did, byte for byte, now
  named `<stem>-male-<k>-T<n>`; and since every Inlay now has a female,
  it also cuts the female on its stock. `check` and `export` say so:

  ```text
  dc-male.dcv: note: operations[op1] `DC` was the male of a two-file inlay pair (format 24); one Inlay now cuts both boards, so this file cuts its male on a male board the size of the stock, as before, and also the female on the stock. Run only its male programs (export --board male), or make one project for the pair from the design and set its male_board
  ```

  Run only its male programs, with `export --board male`. Or make the
  pair one project: in the female project, set the Inlay's `male_board`
  to the male project's stock (its `thickness_mm` at least; its width
  and height only if they differ from the female's stock), copy the
  male's `cut_out` if it had one, and it cuts both boards from then on.

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
  outline at its start plane, `depth_mm − glue_gap_mm` below the male
  board's top.
  Above it, the plug narrows as the pocket narrows going down, so the two
  walls meet along their whole height when the plug is turned over.
- **Floor.** The band is cut flat `plug_depth_mm` below the start plane,
  from the plug's foot out to the band's outer edge.
- **The plug base.** Under the band, the male board holds the plug
  together until the glue dries (Corbin's "Plug Base Thickness"). Less
  than 6 mm of it, `male_board.thickness_mm − (depth_mm − glue_gap_mm +
  plug_depth_mm)`, warns `inlay_plug_base_thin`: use a thicker male
  board. The stock's thickness does not count.

`check` and `export` say what each board cuts:

```text
Inlay op1 `DC` (female board): deepest cut 4.000 mm below the stock top, a flat floor at depth_mm 4.000 mm; V-bit floor ridges 0.400 mm along the rings and up to 0.800 mm at their corners where no clearing pass reaches.
Inlay op1 `DC` (male board): deepest cut 5.000 mm below the male board's top, the plug's start plane 3.000 mm below the male board's top and the surround's flat floor plug_depth_mm 2.000 mm below that; V-bit floor ridges 0.200 mm along the rings and up to 0.400 mm at their corners where no clearing pass reaches; clearing pass `t2` cuts the band's outer edge upright, its inward curves rounded to 10.000 mm; the cut-out frees the plug along the floor's outer edge, its centre 1.587 mm inside the band's edge and at least 6.412 mm out from the design, through the board to 0.200 mm below it.
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
time, not fit: only the waste grows. The female ignores the value, as
it ignores `margin_mm`.

**At the board's edge.** The band only gives the plug room, so when it
runs past the male board (a board cut to the drawing's size, with a wide
margin) it is clipped to the board, and `check` says so (the message
calls the male board the stock):

```text
warning: inlay_band_clipped: operation `op1` (male board): its band runs past the stock's -X (left) and +Y (back) edges, so it is clipped to the board there and is 10 mm wide at its narrowest; the board's edge is the plug's rim there, and a cut-out stops at it
```

With an open edge the edge pass clears the floor right to the board's
edge, its cutting edge on it and never past it, and the board's edge is
the plug's rim there. With a V wall the V-bit runs down the wall to its
top, and the safety rules hold a V-bit to its whole radius, so the band
is clipped that radius inside the board, leaving a strip of board
standing at the edge. Where the band stays on the board nothing changes.

The plug itself is never clipped. Where an edge clips the band, the
design must stand far enough from it for what the band holds there: the
V-bit runs along the plug's foot, `plug_depth_mm × tan(α/2)` out from
the design, and its whole radius must stay on the board; with an open
edge the edge pass's end mill must fit between the foot and the edge;
and a cut-out needs its floor as in [Room](#cutting-the-plug-free). If
the design is nearer than that, `check` refuses the male, names the
edge and the room it needs, and says how far to move the design or how
big a board would fit it centred. With the quick start's bits (the 60°,
6.35 mm V-bit, `plug_depth_mm` 2, the 3.175 mm end mill) that is 4.33
mm; a design closer than about 4 mm to the edge needs a smaller V-bit.

### The mirror, and which way to flip the plug

The plug is glued in face down, so the male cuts the design **mirrored
left to right**, about the vertical line through the middle of its
targets' bounding box. `check` and `export` say so once for each Inlay
that cuts its male, with the line's X:

```text
warning: inlay_male_mirrored: operation `op1` (male board): it is a male inlay, so its design is cut mirrored about X = 55 mm, the middle of its targets, to seat face down in the female
```

Draw the design as it should read in the finished board, and never
mirror it yourself. The program itself is not flipped: its moves are the
mirrored toolpath, in the male board's coordinates.

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
| `tool` | A flat end mill, long enough to go through the male board (see **Flute length** below). Its feed, plunge rate, speed and `step_down_mm` are used; each pass goes one step-down deeper. |
| `tabs` | Holding tabs, as a [Profile's tab request](tabs.md): how many or where, their width along the cut, their top above the bottom of the male board, and their shape. `null` cuts the plug free on the last pass: hold it down another way, with double-sided tape under it. |
| `through_cut_allowance_mm` | How far below the male board the last pass goes, into the spoilboard: 0.2 mm is usual. |

The cut-out is the male's last tool run, after its V-bit, so the
plug is freed only once it is carved; with the same end mill as the
clearing pass the tool is fitted twice, and `check` says
`tool_change_avoidable`, which you can ignore: the order is required.
Its program is named `… cut-out t2`.

**Where it cuts.** The band's floor is flat between the plug's foot and
the band's outer edge: at the floor, the V-bit's flank stands
`plug_depth_mm × tan(α/2)` off the design on the plug's side. The
cut-out runs as far out as the floor reaches, so above the floor it cuts
nothing the carve left standing:

- **With an open edge** the floor runs out to the band's edge itself,
  which the edge pass cut upright, and the cut-out's cutter runs along
  that wall: its centre is its radius inside the band's edge, 1.588 mm
  for the quick start's 3.175 mm end mill, at least `margin_mm` less the
  radius out from the mirrored design (6.412 mm there).
- **With a V-walled edge** the floor stops at the wall's foot,
  `plug_depth_mm × tan(α/2)` inside the band's edge, and the cutter runs
  along that: its centre is `plug_depth_mm × tan(α/2)` plus its radius
  inside the edge. It never reaches the slope, which would leave a sharp
  edge, the full height of the board, standing round the freed piece,
  and it would land on the female's surface.

Either way it follows the band's edge, rounded as it is: round the
design's convex corners on arcs, and over every dip and gap that
`outline_rounding_mm` (or, with an open edge, the edge pass's radius)
fills. The piece it frees is the plug on its backing, with a rim of the
band's floor round it, `plug_depth_mm` below the plug's start plane,
which clears the female's surface by that much when the plug is seated.
A hole in the design (the D's counter in the quick start) is never cut
round: it stays in the plug.

Where the band is clipped to the board's edge (see **At the board's
edge** above), the board's own edge frees the plug, so the cut-out stops
there: it runs from the board's edge round the plug to the board's edge,
its centre its radius inside the edge at each end, so its cut reaches
the edge and the bit never leaves the board. Its tabs go only on those
stretches. With a V-walled edge the band stops short of the board's edge
and the cut-out goes all the way round, as above.

**Room.** The floor ring must be at least the end mill's diameter wide,
so the cutter running along its outer edge stays clear of the plug's
wall; else `check` refuses it, `inlay_cut_out_no_room`, and names the margin
needed. With an open edge it is `margin_mm − plug_depth_mm × tan(α/2)`
wide; with a V wall, `margin_mm − 2 × plug_depth_mm × tan(α/2)`. With the
60° bit and a 2 mm plug depth, a 6.35 mm cut-out needs a margin of at
least 7.505 mm when a clearing pass cuts the edge, and 8.66 mm (use 10)
without one; a 3.175 mm cut-out needs 4.33 mm and 5.48 mm.

**Flute length.** The cut-out goes through the male board, so the end
mill's flutes must be at least the male board's thickness plus
`through_cut_allowance_mm` plus 2 mm long: 14.2 mm for a 12 mm board
with 0.2 mm through. DreamCarve does not know a tool's flute length and
cannot check it. A 1/8 in end mill often has only 12.7 mm (1/2 in); for a
thick male board use one with longer flutes, a 1/4 in end mill with a
wider margin, or `"cut_out": null` and a saw.

**Depth.** The passes start at the band's floor, `depth_mm −
glue_gap_mm + plug_depth_mm` below the male board's top, so none cuts
the air the carve has removed: the first lap is one step-down below the
floor, and each goes a step-down deeper, through the male board to
`through_cut_allowance_mm` below it. The end mill gets there from the
male board's top plus 1 mm at its plunge rate. The tabs' heights count
from the bottom of the male board, as a Profile's do from the stock's, so they must be below the floor:
on a 12 mm board with a 5 mm deep floor, below 7 mm. Each pass below a
tab's top lifts over it.

After the cut-out, cut the tabs with a knife or a fine saw and lift the
plug out. Without a cut-out (`"cut_out": null`), saw round the plug,
through the board, inside the band, between the plug's foot and the
band's outer edge: a bandsaw or scroll saw.

## Clearing passes

Both boards take the [clearing passes](v-carve.md#clearing-passes), as a
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
`step_over_fraction` × the floor's depth below the start plane high, and
higher where the rings turn a corner: up to twice that where the floor
comes to a point (see [V-Carve: the V-bit
floor](v-carve.md#the-v-bit-floor)). `check` prints both heights; with
clearing passes they stand only where no pass reaches. Neither floor
shows in the finished board: the female's is under the glue gap, the
male's is sawn off. But ridges under the plug hold it up, so keep
`glue_gap_mm` above the female's **corner ridge**, the higher figure:
0.8 mm in the quick start, under its 1 mm gap. Keep
`step_over_fraction` at 0.1, or clear the floor with an end mill.

## The fit

Fit is found by testing, as Corbin's fit adjustment procedure does:
cut the female, cut the male, cut the plug free, and press it into the
pocket dry, without glue.

- **It seats with no gap at the surface**, and its backing stops short
  of the female's surface: the fit is right. Glue it.
- **A gap at the surface, all round**, the plug in as far as it goes:
  the plug's face reached the pocket's floor before its walls met the
  pocket's. Raise `glue_gap_mm`, by 0.25 mm at a time, and cut a new
  male. A larger gap makes a shorter plug that seats on its walls first.
- **The plug will not go in**, or stops well short: lower `glue_gap_mm`,
  by 0.25 mm at a time, and cut a new male. The plug gets taller, its
  start plane deeper in the male board, which is Corbin's remedy for a
  plug that is not deep enough to seat.
- **The top looks right, but a test pair cut in half shows a gap under
  the plug** bigger than you want: lower `glue_gap_mm`; the walls still
  meet, and the glue line under the plug is thinner. Keep it above the
  female's corner ridge (see [Clearing passes](#clearing-passes)), or the
  plug rests on the floor's ridges.

Only the male changes: the female's pocket depends on `depth_mm` alone.
So after changing `glue_gap_mm`, export the male board alone, `dreamcarve
export dc.dcv --board male`, and cut it on a new board (see [Cutting one
board again](#cutting-one-board-again)).
Once you have found the value for a bit, a pair of woods and your
machine, keep it. A fit that needs a glue gap close to `depth_mm`
points at the bit: its real angle or tip is not what the tool says.

A press, or a clamp with a flat caul on each side, closes the fit
further: hardwood compresses a little as the walls are forced together.

### Corners and points

Both boards carve every corner to a point: a V-bit cannot cut a sharp
corner below the surface, so at each depth the pocket is the outline
shrunk by the wall's slope, and a sharp point shrinks back along its
length much more than a straight side moves in. In the quick start's
"DC", the serifs' tips and the C's pointed terminals end, 3 mm down,
well short of where they are drawn. The plug's points are the same shape
turned over, so the two still meet; but the last few millimetres of each
point are only a shallow V. That is expected: the points are sharp at the surface, where
they show, and they are the part to treat gently when you saw and plane.
Parts of a design narrower than the bit can reach into come out as fine
V's on both boards.

A notch in the design, such as the narrow V where a serif M's stem meets
its diagonal, is a point of the surround the male carves: its V-bit runs
into the notch right to its tip, at every angle, so the plug's wall there
leans back exactly as the pocket's does. Wood left standing in a notch
would hold the whole plug up.

### Parts that nearly touch

Two parts of a design that come within a hair of each other, such as
letters whose serifs almost meet or a motif whose point stops just short
of a frame, leave a gap no V-bit can carve. The female would leave a
knife-edge wall standing to the top of the board between the two
pockets, the male would cut no groove between the two parts of its plug,
and the plug would stand proud on that wall by up to `depth_mm −
glue_gap_mm`.

So both boards cut two parts that come closer than the **merge
distance** as one shape: the female cuts one pocket, the male one plug,
and they still fit. The distance is 0.05 mm. A gap of about 0.02 mm or
less cannot be carved at all: curves are carved as short straight
chords up to `flatten_tolerance_mm` off the drawn line, so two outlines
closer than twice it can meet in the chords, and the V-bit's tip keeps
half a unit in the last decimal place from each wall. A little wider,
the V-bit still cannot reach far enough into the narrow wedge on either
side of the gap, and the plug can stand up to 1.6 mm proud there; at
0.05 mm it seats. Each pair warns
`inlay_parts_merged`, naming the two shapes, the gap and where it is, as
drawn, once for both boards. Only the gap
fills: both parts keep their shape everywhere else. Parts the merge
distance or more apart are cut as drawn, with the wall between them;
0.1 mm apart, the pair seats as any other. A plain V-Carve or Pocket
never merges parts.

One outline that comes that near itself across the outside, where two
overlapping letters became one shape, is merged across the gap the same
way, and `inlay_parts_merged` names the shape once ("… comes within …
mm of itself at …").

A hole of the design narrower than the merge distance all along, a
hairline where two overlapping letters leave a sliver, is the same
thing the other way round: the female would leave a wall that thin
standing to the stock top, and the male could cut no groove for it.
Both boards cut it as part of the shape round it, and warn
`inlay_hole_filled` with its width and where it is.

If the parts are meant to touch, nothing needs doing. If they are meant
to stay apart, move them at least the distance the warning gives apart
in the drawing.

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

Every Inlay program passes the [safety](safety.md) rules, each against
its own board: the female's against the stock, the male's against the
male board, its thickness, its size and its origin. So a male program
may go no lower than the male board's bottom (the cut-out's own
allowance below it) and stays on the male board's rectangle. Rule 14
(S17) holds the V-bit's cone to its board's own material: the
design for the female, its near parts merged as the generator merges
them; the surround (the band and the design's holes) for the male, with the start plane `depth_mm − glue_gap_mm` below the male board's top and
the flank engaged from that top. When a clearing pass cuts the male's
band edge upright, the board beyond that edge stands to the male board's
top, so the V-bit's cone is also held inside the band right up to that
top.
The band, its rounding and which pass cuts its edge are worked out again
from the project. A clearing pass is held as a Pocket of its floor. A
male's cut-out is held to the band's flat floor ring and to its own path
along the ring's outer edge, both worked out again from the project: the
end mill's centre stays on that path, its whole cutter stays clear of
both the ring's edges at every depth, it goes no deeper than its own
`through_cut_allowance_mm` below the male board, and it never plunges
into one of its tabs.

## Errors

These stop `check` and `export`.

| Code | Message says | What to do |
| --- | --- | --- |
| `operation_value_unset` | "operations[…].depth_mm is not set", and the same for `glue_gap_mm`, `plug_depth_mm`, and once the male board's thickness is set `margin_mm` | Set the value. |
| `out_of_range` | `depth_mm` or `plug_depth_mm` is not above 0, or `glue_gap_mm` is below 0 | Set a value in range. |
| `inlay_glue_gap_not_below_depth` | "… has glue_gap_mm … mm, not below its depth_mm … mm; the male starts depth_mm − glue_gap_mm below the stock top, so its plug would have no wall to seat …" | Make the glue gap smaller than the depth. |
| `inlay_margin_positive` | "… is a male inlay with margin_mm … mm; its surround is carved that far beyond the design, so it must be above 0 …" | Set a margin above 0; 5 to 8 mm. |
| `inlay_male_board_size` | "… has male_board.width_mm … mm; the male board's size must be above 0. Set it to null to use the stock's" (or `height_mm`) | Set the male board's size above 0, or `null` for the stock's. |
| `inlay_male_board_thickness` | "… has male_board.thickness_mm … mm; measure the male board and set its thickness, above 0" | Set the male board's measured thickness. |
| `inlay_male_boards_differ` | "… cuts its male on a male_board other than operation …'s; every Inlay of a project cuts its male on the one male board, so give them the same male_board" | Give every Inlay the same `male_board`. See [Two Inlays](#two-inlays). |
| `inlay_outline_rounding` | "… has outline_rounding_mm … mm; the male's band is rounded by that radius, so it must be above 0 …" | Set a radius above 0, or `null` for none. |
| `v_carve_needs_v_bit` | "operation … carves with tool …, an end mill; a carve's depth follows the angle of a V-bit, so use a V-bit" | Give the Inlay a V-bit. |
| `v_bit_angle_range` | "tool … has an included angle of … degrees; …" | Fix the tool's `included_angle_deg`. |
| `v_bit_cone_exceeded` | "operation … engages tool … … mm deep …"; for the male "… from the stock top (its start plane, depth_mm − glue_gap_mm below the stock top, plus its plug_depth_mm; the plug above the start plane is never cleared)", the stock top being the male board's | See [Choosing the bit](#choosing-the-bit). The female's is at `depth_mm`, the male's at `plug_depth_mm`; with both boards cut, both can be reported. |
| `inlay_cut_out_no_room` | "… cuts its plug out with tool …, … mm across, along the outer edge of the band's flat floor; with margin_mm … mm that floor is narrower than the tool … Raise margin_mm to at least … mm …, or use a narrower end mill" | Raise `margin_mm`, or use a narrower end mill. |
| `cutter_not_flat` | "… cuts its plug's cut-out with tool …, a V-bit; …" at `cut_out.tool` | Give the cut-out a flat end mill. |
| (none) | "operation …'s male design is … mm from the stock's … edge, which clips its band, but the band needs … mm there …; move the design at least … mm away from that edge, or make the stock at least … mm wide (or long) with the design centred", the stock being the male board | Move the design, or use a bigger male board, by at least what it says; or a smaller V-bit or end mill. See **At the board's edge** above. |

The clearing errors (`clearing_needs_end_mill`, `clearing_dangling_tool`)
are as a [V-Carve's](v-carve.md#errors). An Inlay has no start depth, so
the start depth errors never name one, and it always has a floor, so
`clearing_needs_flat_depth` never does either. Export also stops when
neither the operation nor its V-bit sets `step_over_fraction`.

## Warnings

| Code | When |
| --- | --- |
| `inlay_male_board_unset` | The male board has no thickness set, so only the female is cut. Measure the male board and set `male_board.thickness_mm`. |
| `inlay_male_mirrored` | Every Inlay that cuts its male, once: a note giving the mirror line. |
| `inlay_plug_base_thin` | The male leaves less than 6 mm of the male board under its band. |
| `inlay_band_edge_v_wall` | A male has clearing passes, but none fits its band's floor, so the V-bit cuts the band's outer edge as a sloped wall; it names the margin that would let a pass cut it upright. |
| `inlay_band_clipped` | The male's band runs past the male board's edge, so it is clipped to the board there; it names the edges and the narrowest band left. |
| `inlay_parts_merged` | Two parts of the design come closer than the merge distance, so both boards cut them as one shape ([Parts that nearly touch](#parts-that-nearly-touch)). |
| `inlay_hole_filled` | A hole of the design is narrower than the merge distance everywhere, so both boards cut it as part of the shape round it ([Parts that nearly touch](#parts-that-nearly-touch)). |
| `vcarve_target_open` | A target is an open path; it is skipped. |
| `clearing_no_room`, `clearing_not_descending` | As for a [V-Carve](v-carve.md#warnings). |

[Warnings](warnings.md) says what to do about each.

## Limits

- 2D inlays on flat boards only; no inlays on curved surfaces.
- Every male of a project is cut on one male board.
- The cut-out frees the plug on its backing; the backing is not removed:
  that is a saw and a plane, after the glue-up.
- The cut-out has no entry ramp or lead: it plunges at the plunge rate,
  a step-down at a time.
- The bit is an ideal point; a real tip has a small flat. The fit
  procedure absorbs it, as it absorbs a bit whose angle is a little off.
