# Tabs

When a Profile cuts a part all the way through, the part comes loose on the
last pass. A loose part can be thrown by the bit or catch it and spoil the
edge. **Tabs** are short bridges of material left uncut along the outline.
They hold the part in the stock until the program ends. You cut them by
hand afterwards.

Tabs are set per operation, in the Profile's `tabs` field, and laid on
each outline the operation cuts:

```json
"tabs": {
  "placement": { "kind": "count", "count": 4 },
  "width_mm": 8.0,
  "height_mm": 3.0,
  "shape": "rect",
  "on_holes": false
}
```

Set `"tabs": null` for no tabs.

## Settings

| Field | Meaning |
| --- | --- |
| `placement` | Where the tabs go. See [Placement](#placement). |
| `width_mm` | Length of each tab along the outline, as it is left on the part. |
| `height_mm` | Height of the tab top above the stock bottom. A rect tab is this thick all along; a smooth tab is this thick at its middle. |
| `shape` | `"rect"` (square shoulders) or `"smooth"` (a triangle: straight ramps up and down). |
| `on_holes` | `false`: holes get no tabs, so each hole's slug comes loose. `true`: holes get the tabs too. See [Parts with holes](#parts-with-holes). `init` writes `false`. |

The coaster's tabs are 8 mm wide and 3 mm tall, in the quick start's
11.8 mm plywood: each pass deeper than 3 mm above the stock bottom rises to
Z 3 mm over each tab. With work zero at the stock top, that is Z −8.8 in
the program.

## Placement

- `{ "kind": "count", "count": 4 }` spaces that many tabs evenly around
  each outline. Count from 1 to 255.
- `{ "kind": "at_distances", "distances_mm": [10.0, 60.0] }` puts a tab
  centred at each distance, in millimetres, along the bit's path from where
  the path starts. A distance longer than the path wraps around.

The path starts where DreamCarve chooses, which is not shown before you
export, so `count` is the easier choice. The distances are
measured from that start even when the cut begins elsewhere: a
[lead](profile.md#leads) arrives away from corners, and a ramp or helix
that has no room beside a tab moves the start of the cut to a stretch
between tabs ([Profile: entry](profile.md#entry)). Neither moves a tab.

On an open path cut `on_line` ([Profile: open paths](profile.md#open-paths))
the distances are measured from the path's first vertex as drawn,
whichever end the cut starts at. A tab near an end stops at it instead
of running on round the path.

Every outline the operation cuts gets the tabs, except its holes unless
`on_holes` is `true` ([Parts with holes](#parts-with-holes)), and except
that an outline too short for them gets fewer
([Outlines too short for the tabs](#outlines-too-short-for-the-tabs)).

## What the bit does over a tab

The bit must clear the tab with its whole width, so the stretch of path that
rises over one tab is the tab width plus the bit diameter: 8 + 6.35 =
14.35 mm for the coaster.

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
  heavy. Smooth tabs that touch or overlap join into one: the bit ramps
  up, holds the tab top until the last tab, and ramps down.

Passes at or above the tab top are not changed. Only the passes that cut
below it rise over the tabs.

## Outlines too short for the tabs

Tabs need room. On a small hole or a short outline, the tab stretches
(tab width plus bit diameter each) can add up to most of the bit's path.
Then the last pass barely reaches full depth, or never does, and the part
or hole stays attached all the way round.

So when the tabs on one outline would cover more than half of the bit's
path, that outline gets the most tabs that cover at most half of it,
possibly none:

- With `count`, fewer tabs, spaced evenly round the outline.
- With `at_distances`, the first distances in your list, as many as fit.

Other outlines keep all their tabs. `check` and `export` warn for each
outline that gets fewer. The warning names the shape, the length of the
bit's path, your tab width and the bit diameter, how much of the path
each tab keeps raised (tab width plus bit diameter, because the bit is
round), the total and its share of the path, how many tabs are cut, and
the width at which all of them would fit. For example, four 8 mm tabs on
a 20 mm square hole with `on_holes` set, cut with a 6.35 mm bit:

```text
warning: tabs_cover_most_of_contour: operation `op1`: tabs on `small-hole` (instance `i3`, profile 0) would cover more than half of its tool path: each of the 4 tabs is 8 mm wide, and with the 6.35 mm bit each one keeps 14.35 mm of the tool path raised, 57.4 mm in total on a 54.6 mm path (105.1%), so the part or hole would not cut free. Only 1 of them is cut there. To keep all 4, make them at most 0.47 mm wide.
```

To keep every tab, make them narrower, or use a smaller bit. Exactly half
is still allowed: the rule is more than half.

## Parts with holes

A hole's slug is waste, so by default a Profile cuts its holes free and
puts tabs only on the part. One operation then cuts a part and all its
holes, holes first: a star with a small hole in its centre takes one
operation, with four tabs on the star and none in the hole.

1. Draw the star and its hole, as one path or two; `init` targets both.
2. Keep the operation `"side": "outside"`. DreamCarve sees that the hole
   lies inside the star, so it cuts the hole from inside, at the size
   drawn, and before the star, while the star is still held by the
   stock.
3. Set the tabs you want on the star. `on_holes` stays `false`.

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
  "tabs": {
    "placement": { "kind": "count", "count": 4 },
    "width_mm": 8.0,
    "height_mm": 3.0,
    "shape": "smooth",
    "on_holes": false
  },
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

Set `"on_holes": true` to hold the slugs too, for example to keep a
disc cut from the middle of a ring. Each hole then gets the tabs, or as
many as fit
([Outlines too short for the tabs](#outlines-too-short-for-the-tabs)).
A project from before format version 9 opens with `"on_holes": true`,
because every outline had tabs then, so it cuts as it did.

Splitting a part and its holes into two operations still works, but
operations run in the order they are listed, so put the holes first.
Cut after the part, a hole is cut in a part held only by its tabs, or in
a loose part if it has none, which `check` warns as
`cut_inside_freed_part`. A hole cut on its own operation is not a hole
to DreamCarve (holes are found only among one operation's targets), so
cut it `"inside"`.

Placing tabs on the part by hand, one at a time, is planned
([#105](https://github.com/grayzu/dreamcarve/issues/105)).

## Warnings

These still give a safe program. `check` and `export` print each one as a
sentence, after the operation's id.

| Warning starts with | Meaning |
| --- | --- |
| "tabs on ... touch or overlap" | Tabs on one outline touch or overlap, so they were joined into one longer tab. Use fewer or narrower tabs, or check that the outline is not too short for them. |
| "tabs on ... would cover more than half of its tool path" | The outline is too short for its tabs, so it gets fewer, possibly none. See [Outlines too short for the tabs](#outlines-too-short-for-the-tabs). |
| "the tab top is ... but the deepest pass stops at ..." | No pass goes below the tab top, so the tabs change nothing. The cut is shallower than the stock, or the tabs are taller than the depth reaches. |
| "the tab top is ... higher than the ... stock top" | The tab top is above the stock top. The tabs still apply: over each tab, every pass lifts clear of the stock, leaving its full thickness. |

## Removing tabs

Cut each tab with a sharp knife, a chisel, or a flush-cut saw, then sand
the stub flush. Tabs on thin or brittle material can be lower; tabs on a
heavy part can be wider or more numerous.
