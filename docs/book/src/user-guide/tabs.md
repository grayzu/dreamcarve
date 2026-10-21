# Tabs

When a Profile cuts a part all the way through, the part comes loose on the
last pass. A loose part can be thrown by the bit or catch it and spoil the
edge. **Tabs** are short bridges of material left uncut along the outline.
They hold the part in the stock until the program ends. You cut them by
hand afterwards.

A tab belongs to the part, not to an operation. Each one is placed on one
outline of one shape, at one point, and has its own width, height and
shape. Every enabled Profile that cuts that outline rises over it. The
project keeps its tabs in one list, `tabs`, after the operations. A tab
moves with its shape: move, rotate, mirror or scale the instance and the
tab goes with it.

You place tabs with `dreamcarve tabs place`, look at them with `tabs
list`, and remove them with `tabs clear`. `init` writes the coaster's four
tabs as a [tab request](#tab-requests) on its Profile, which is cut as the
tabs it places. The first `tabs` command that saves the project writes
them as placed tabs.

## Placing tabs

### Evenly round each outline

```sh
dreamcarve tabs place coaster.dcv op1 --count 4
```

This spaces four tabs evenly along each tool path Profile `op1` cuts, and
saves the project. It prints one line per new tab, then a total:

```text
Placed `tab1` on `coaster` (instance `i1`, profile 0) 35.343 mm along it: 8 mm wide, top 3 mm above the stock bottom, rect
Placed `tab2` on `coaster` (instance `i1`, profile 0) 106.029 mm along it: 8 mm wide, top 3 mm above the stock bottom, rect
Placed `tab3` on `coaster` (instance `i1`, profile 0) 176.715 mm along it: 8 mm wide, top 3 mm above the stock bottom, rect
Placed `tab4` on `coaster` (instance `i1`, profile 0) 247.4 mm along it: 8 mm wide, top 3 mm above the stock bottom, rect
4 tab(s) placed; coaster.dcv saved with 4 tab(s).
```

| Option | Meaning |
| --- | --- |
| `--count N` | Tabs per tool path, spaced evenly. 1 to 255. |
| `--width MM` | Length of each tab along the outline, as it is left on the part. Default 8. |
| `--height MM` | Height of the tab top above the stock bottom. A rect tab is this thick all along; a smooth tab is this thick at its middle. Default 3. |
| `--shape rect\|smooth` | `rect` (square shoulders, the default) or `smooth` (a triangle: straight ramps up and down). See [What the bit does over a tab](#what-the-bit-does-over-a-tab). |
| `--on-holes` | Put tabs on holes too. Without it holes get none. See [Parts with holes](#parts-with-holes). |

`--count` follows the rules below: no tabs on holes unless `--on-holes`,
and fewer tabs on an outline too short for them, but at least one on a
part ([Outlines too short for the tabs](#outlines-too-short-for-the-tabs)).
The tabs are placed for that Profile's bit and side: each one is on the
tool path, the bit's radius off the outline for an Inside or Outside cut.

On a project `init` wrote, the Profile already asks for four tabs, and
`tabs place` places that request first ([What every `tabs place`
does](#what-every-tabs-place-does)), so the command above places just
those four. To have three instead, clear them first:

```sh
dreamcarve tabs clear coaster.dcv
dreamcarve tabs place coaster.dcv op1 --count 3
```

### At distances along the outline

```sh
dreamcarve tabs clear coaster.dcv
dreamcarve tabs place coaster.dcv op1 --at 0,70.686
```

This puts one tab at each distance, in millimetres, along every outline
the operation cuts, measured from the outline's first vertex as you drew
it. The distance is measured on the shape as drawn, before any scale in
its instance's transform. A distance longer than a closed outline wraps
round; on an open path it stops at the ends. Holes are skipped unless
`--on-holes` is given. `--width`, `--height` and `--shape` work as with
`--count`.

These tabs are on the outline itself, and they are cut as placed: none is
dropped because the outline is short. `tabs place` warns when they touch
or overlap (`tabs_overlap`) or cover more than half of a tool path
(`tabs_cover_most_of_contour`). The coaster's circle starts at its
rightmost point, so the two tabs above go there and a quarter of the way
round, at its bottom.

### An operation's own request

```sh
dreamcarve tabs place coaster.dcv op1
```

With neither `--count` nor `--at`, `tabs place` places the operation's
[tab request](#tab-requests) and sets it to `null`. An operation with no
request is a usage error (exit code 2):

```text
error: operation `op1` has no tab request to place; give --count or --at
```

### What every `tabs place` does

- It adds tabs; it never removes any. Run `tabs clear` first to replace
  tabs. A tab exactly like one already there, on the same outline at the
  same point with the same size and shape, is not added again.
- New tabs take the next free id: `tab5` after `tab4`, even when `tab2`
  was removed.
- It first places the tab request of every enabled Profile, so the file
  it saves holds placed tabs only.
- It works only on a Profile. Another kind fails (exit code 1), and so
  does an operation the project does not have:

  ```text
  error: operation `o2` is a pocket operation; only a Profile cuts tabs
  error: operation `op9` is not in coaster.dcv
  ```

## Listing tabs

```sh
dreamcarve tabs list coaster.dcv
```

```text
ID    INSTANCE  PROFILE  AT_MM    X_MM    Y_MM    WIDTH_MM  HEIGHT_MM  SHAPE  CUT_BY
tab1  i1        0        35.343   84.065  15.935  8         3          rect   op1
tab2  i1        0        106.029  15.935  15.935  8         3          rect   op1
tab3  i1        0        176.715  15.935  84.065  8         3          rect   op1
tab4  i1        0        247.4    84.065  84.065  8         3          rect   op1
1 operation(s) hold a tab request, listed as the tabs it places; any `tabs` command that saves the project writes them.
```

| Column | Meaning |
| --- | --- |
| `ID` | The tab's id, for `tabs clear --tab`. |
| `INSTANCE`, `PROFILE` | The shape and which of its outlines, counted from 0. |
| `AT_MM` | How far along the outline, from its first vertex as drawn, the tab meets it. |
| `X_MM`, `Y_MM` | The tab's point, in the shape's own coordinates (its definition), not the stock's. |
| `WIDTH_MM`, `HEIGHT_MM`, `SHAPE` | The tab's size and shape. |
| `CUT_BY` | The enabled Profiles that rise over the tab, or `-` for none. |

The last line appears when an enabled Profile still holds a tab request,
as the coaster's does after `init`. The table shows those tabs as they
would be placed; `list` does not save the file. A project with no tabs
prints `coaster.dcv has no tabs.`

## Removing tabs

```sh
dreamcarve tabs clear coaster.dcv              # every tab
dreamcarve tabs clear coaster.dcv op1          # the tabs on the shapes op1 targets
dreamcarve tabs clear coaster.dcv --tab tab2   # one tab; repeat --tab for more
```

`tabs clear` saves the project and prints how many tabs it removed and
how many are left. Like `tabs place`, it first places every enabled
Profile's tab request, so clearing all tabs also clears what `init`
asked for. A `--tab` id the project does not have fails (exit code 1)
and changes nothing.

## A placed tab in the file

```json
"tabs": [
  {
    "id": "tab1",
    "instance": "i1",
    "profile": 0,
    "x_mm": 84.0648714595208,
    "y_mm": 15.935129303418655,
    "width_mm": 8.0,
    "height_mm": 3.0,
    "shape": "rect"
  }
]
```

| Field | Meaning |
| --- | --- |
| `id` | The tab's id. Two tabs may not share one. |
| `instance` | The id of the instance whose outline holds the tab. |
| `profile` | Which outline of that instance's definition, counted from 0. |
| `x_mm`, `y_mm` | The tab's point, in the definition's coordinates: the shape as drawn, before the instance's transform. |
| `width_mm` | Length of the tab along the outline. More than 0. |
| `height_mm` | Height of the tab top above the stock bottom. More than 0. |
| `shape` | `"rect"` or `"smooth"`. |

You can edit a tab in the file: change its size or shape, or move its
point. The point need not be exactly on the outline. A Profile centres the
tab at the point of its tool path nearest it. `tabs place --at` writes
points on the outline; `--count` and requests write points on the tool
path. A tab on an instance or outline that does not exist is an error.

## Tab requests

A Profile's `tabs` field is a **tab request**: tabs to place on every
outline the Profile cuts. `init` writes one:

```json
"tabs": {
  "placement": { "kind": "count", "count": 4 },
  "width_mm": 8.0,
  "height_mm": 3.0,
  "shape": "rect",
  "on_holes": false
}
```

`null` means no request. A request is cut exactly as the tabs it places,
so files from before placed tabs (format version 11) cut as they did,
with nothing for you to do. Any `dreamcarve tabs` command that saves the
project places the request of every enabled Profile and sets it to
`null`. A disabled Profile keeps its request until you enable it, or
place it by name with `tabs place <dcv> <op>`. `check`, `export`,
`export --post` and `tools add` leave requests as they are.

| Field | Meaning |
| --- | --- |
| `placement` | `{ "kind": "count", "count": 4 }` spaces that many tabs evenly along each tool path (1 to 255), as `--count` does. `{ "kind": "at_distances", "distances_mm": [10.0, 60.0] }` puts a tab centred at each distance along the bit's path from where the path starts; a distance longer than the path wraps around. |
| `width_mm`, `height_mm`, `shape` | As for a placed tab. |
| `on_holes` | `false`: holes get no tabs, so each hole's slug comes loose. `true`: holes get the tabs too. `init` writes `false`. |

`at_distances` measures along the bit's path, not the outline, from a
start DreamCarve chooses and does not show before you export. On an open
path cut `on_line` ([Profile: open paths](profile.md#open-paths)) it
measures from the path's first vertex as drawn, whichever end the cut
starts at. To put tabs at known places, use `tabs place --at`, which
measures along the outline from its first vertex.

## Which cuts rise over a tab

Every enabled Profile that cuts the tab's outline rises over it: an
Outside and an Inside Profile of the same outline both do, whichever
placed it. A Profile crosses a tab when its tool path passes within the
bit's radius plus half the tab width of where the tab meets the outline.
A path farther off than that, such as an Outside cut past a narrow notch
the bit cannot reach into, does not cut round the tab, so it does not
rise. A tab no enabled Profile crosses warns `tab_not_crossed`
([Warnings](#warnings)).

Only the tab's own outline counts. A Pocket, an Engrave, or a Profile of
another shape that passes over the spot does not rise.

Tabs are cut where they are. A [lead](profile.md#leads) arrives away from
corners, and a ramp or helix that has no room beside a tab moves the start
of the cut to a stretch between tabs ([Profile: entry](profile.md#entry)).
Neither moves a tab. Changing the Profile's bit or side does not move a
placed tab either.

## What the bit does over a tab

The bit must clear the tab with its whole width, so the stretch of path that
rises over one tab is the tab width plus the bit diameter: 8 + 6.35 =
14.35 mm for the coaster. It is centred on the point of the path nearest
the tab.

- **Rect.** At the start of the stretch the bit moves straight up to the tab
  top at the plunge rate, cuts along the stretch at the tab top at the feed
  rate, and moves straight down at the plunge rate.
- **Smooth.** The tab left on the part is a triangle: `width_mm` long at
  its base and `height_mm` tall at its middle. The bit ramps up in a
  straight line along the path over half the tab width, holds the tab top
  for one bit diameter, and ramps back down over the other half. For the
  coaster's 8 mm tabs and 6.35 mm bit, that is 4 mm up, 6.35 mm level,
  and 4 mm down. The bit never stops moving along the path, which leaves
  fewer marks. Every move is at the feed rate, except a ramp down that
  would go down faster than the plunge rate: its feed rate is lowered just
  enough that it does not ([Safety](safety.md), rule 11). With the
  quick start's 1500 mm/min feed, the coaster's 3 mm ramp down over 4 mm
  would go down at 900 mm/min; with its 500 mm/min plunge rate the ramp
  runs at 833 mm/min.

  A smooth tab has half the cross-section of a rect tab of the same width
  and height, so it holds less. Make it wider or taller if the part is
  heavy.

The coaster's tabs are 8 mm wide and 3 mm tall, in the quick start's
11.8 mm plywood: each pass deeper than 3 mm above the stock bottom rises to
Z 3 mm over each tab. With work zero at the stock top, that is Z −8.8 in
the program.

Each tab has its own height. A pass rises over a tab only when it cuts
below that tab's top; passes at or above it are not changed. Tabs whose
stretches touch or overlap join into one, with a `tabs_overlap` warning:
the joined stretch rises to the highest top among them, and it is smooth
only if all of them are.

## Outlines too short for the tabs

Tabs need room. On a small hole or a short outline, the tab stretches
(tab width plus bit diameter each) can add up to most of the bit's path.
Then the last pass barely reaches full depth, or never does, and the part
or hole stays attached all the way round.

So when `tabs place --count`, or a tab request, would put tabs covering
more than half of the bit's path on one outline, that outline gets the
most tabs that cover at most half of it:

- With a count, fewer tabs, spaced evenly round the outline.
- With a request's `at_distances`, the first distances in the list, as
  many as fit.

A part's outline always keeps at least one tab. Cut fully free, a small
part can be thrown by the bit, which is worse than a part held too
firmly. If even one tab covers more than half of the path, that one is
cut anyway, and the warning says the part may need cutting free by hand:
cut through what is left of the outline with a knife or saw. A hole with
`--on-holes` (or `on_holes` set) has no such floor: its slug is waste, so
it may get no tabs at all.

Other outlines keep all their tabs. The warning names the shape, the
length of the bit's path, your tab width and the bit diameter, how much of
the path each tab keeps raised (tab width plus bit diameter, because the
bit is round), the total and its share of the path, how many tabs are
cut, and the width at which all of them would fit. For example, four 8 mm
tabs on a 20 mm square hole with `on_holes` set, cut with a 6.35 mm bit:

```text
warning: tabs_cover_most_of_contour: operation `op1`: tabs on `small-hole` (instance `i3`, profile 0) would cover more than half of its tool path: each of the 4 tabs is 8 mm wide, and with the 6.35 mm bit each one keeps 14.35 mm of the tool path raised, 57.4 mm in total on a 54.6 mm path (105.1%), so the part or hole would not cut free. Only 1 of them is cut there. To keep all 4, make them at most 0.47 mm wide.
```

`tabs place` prints the warning when it places the tabs, and `check` and
`export` print it while the tabs are still a request. Tabs already
placed are never cut down: you placed them, so every one is cut. When
they cover more than half of a path, `check` and `export` warn the same
way, saying "All of them are cut, as placed". Remove some with `tabs
clear --tab`, or place narrower ones.

To keep every tab, or to let a part's one tab leave room to cut free,
make the tabs narrower, or use a smaller bit. Exactly half is still
allowed: the rule is more than half.

## Parts with holes

A hole's slug is waste, so a Profile cuts its holes free, and `tabs place`
puts tabs only on the part unless you give `--on-holes`. One operation then
cuts a part and all its holes, holes first: a star with a small hole in
its centre takes one operation, with four tabs on the star and none in the
hole.

1. Draw the star and its hole, as one path or two; `init` targets both.
2. Keep the operation `"side": "outside"`. DreamCarve sees that the hole
   lies inside the star, so it cuts the hole from inside, at the size
   drawn, and before the star, while the star is still held by the
   stock.
3. Place the tabs: `dreamcarve tabs place star.dcv op1 --count 4 --shape
   smooth`. They go on the star only.

```json
{
  "kind": "profile",
  "id": "op1",
  "name": "Star",
  "enabled": true,
  "targets": [ "i1", "i2" ],
  "tool": "t1",
  "side": "outside",
  "depth_mm": 12.0,
  "step_down_mm": null,
  "direction": "climb",
  "through_cut_allowance_mm": 0.2,
  "safe_z_mm": 5.0,
  "tabs": null,
  "machining_order": "full_depth_first",
  "entry": { "ramp": { "angle_deg": 15.0 } },
  "lead": null,
  "corner_relief": "none"
}
```

A hole is an outline inside another outline of the same operation's
targets. A part inside a hole (an island, such as a coaster cut from a
frame's opening) is a part again and gets tabs; a hole in that part does
not.

Give `--on-holes` to hold the slugs too, for example to keep a disc cut
from the middle of a ring. Each hole then gets the tabs, or as many as fit
([Outlines too short for the tabs](#outlines-too-short-for-the-tabs)).
A placed tab on a hole is always cut, however it was placed, and `check`
and `export` warn `tab_on_hole`, because a held slug is usually waste you then have to free
by hand. If you did not mean to keep the slug, remove the tab:

```text
warning: tab_on_hole: operation `op1`: tab `tab3` is on `Shape` (instance `i1`, profile 2), which this operation cuts as a hole, so the tab holds the hole's slug in place; the slug is usually waste, so remove the tab (`dreamcarve tabs clear --tab tab3`) unless you mean to keep the slug
```

A project from before format version 9 opens with `"on_holes": true` in
its request, because every outline had tabs then, so it cuts as it did.

Splitting a part and its holes into two operations still works, but
operations run in the order they are listed, so put the holes first.
Cut after the part, a hole is cut in a part held only by its tabs, which
is less rigid than the uncut stock: the part can flex or chatter, and the
cut can break a tab. `check` warns this as `cut_inside_tab_held_part`
(and as `cut_inside_freed_part` when the part has no tabs, so is loose).
Any later cut in the part deeper than half the stock warns the same way;
a lighter one, such as a shallow engraving, does not.
A project from before format version 9 that cut the part first and its
hole second warns so when opened: move the hole's operation first, or
add the hole to the part's operation and delete its own. A hole cut on
its own operation is not a hole to DreamCarve (holes are found only
among one operation's targets), so cut it `"inside"`.

## Warnings

These still give a safe program. `check` and `export` print each one as a
sentence, after the operation's id where there is one; `tabs place`
prints those it finds while placing.

| Warning starts with | Meaning |
| --- | --- |
| "tabs on ... touch or overlap" | Tabs on one outline touch or overlap, so they were joined into one longer tab. Use fewer or narrower tabs, or check that the outline is not too short for them. |
| "tabs on ... would cover more than half of its tool path" | The outline is too short for its tabs. Placed with a count or from a request, it gets fewer, possibly none; placed tabs are all cut. See [Outlines too short for the tabs](#outlines-too-short-for-the-tabs). |
| "the tab top is ... but the deepest pass stops at ..." | No pass goes below the tab top, so the tabs change nothing. The cut is shallower than the stock, or the tabs are taller than the depth reaches. |
| "the tab top is ... higher than the ... stock top" | The tab top is above the stock top. The tabs still apply: over each tab, every pass lifts clear of the stock, leaving its full thickness. |
| "tab ... is on ..., which this operation cuts as a hole" (`tab_on_hole`) | A tab is on a hole, so it holds the slug. See [Parts with holes](#parts-with-holes). |
| "tab ... on ... is crossed by no enabled Profile operation" (`tab_not_crossed`) | No enabled Profile rises over the tab, so it holds nothing: no enabled Profile cuts that outline (it is cut only by a Pocket or a disabled Profile, or not at all), or every one that does runs too far from the tab. Cut the outline with a Profile, move the tab, or remove it with `dreamcarve tabs clear <dcv> --tab <id>`. |

`tab_not_crossed` reads, in full:

```text
warning: tab_not_crossed: tab `tab3` on `Shape` (instance `i1`, profile 2) is crossed by no enabled Profile operation, so it holds nothing; cut that outline with a Profile, or remove the tab (`dreamcarve tabs clear --tab tab3`)
```

## Cutting the tabs off

Cut each tab with a sharp knife, a chisel, or a flush-cut saw, then sand
the stub flush. Tabs on thin or brittle material can be lower; tabs on a
heavy part can be wider or more numerous.
