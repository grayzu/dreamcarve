# Tabs

When a Profile cuts a part all the way through, the part comes loose on the
last pass. A loose part can be thrown by the bit or catch it and spoil the
edge. **Tabs** are short bridges of material left uncut along the outline.
They hold the part in the stock until the program ends. You cut them by
hand afterwards.

Tabs are set per operation, in the Profile's `tabs` field:

```json
"tabs": {
  "placement": { "kind": "count", "count": 4 },
  "width_mm": 8.0,
  "height_mm": 3.0,
  "shape": "rect"
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
export, so `count` is the easier choice in phase 1.

Every outline the operation cuts gets the tabs, holes included.

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
  fewer marks. Every move is at the feed rate.

  A smooth tab has half the cross-section of a rect tab of the same width
  and height, so it holds less. Make it wider or taller if the part is
  heavy. Smooth tabs that touch or overlap join into one: the bit ramps
  up, holds the tab top until the last tab, and ramps down.

Passes at or above the tab top are not changed. Only the passes that cut
below it rise over the tabs.

## Tabs that cover most of an outline

Tabs need room. On a small hole or a short outline, the tab stretches
(tab width plus bit diameter each) can add up to most of the bit's path.
Then the last pass barely reaches full depth, or never does, and the part
or hole stays attached all the way round.

When the tabs on one outline cover more than half of the bit's path,
`check` and `export` warn. The warning names the shape, the length of the
bit's path, the tab total and its share of the path, and suggests a tab
count or width that would cover at most half. For example, the default
four 8 mm tabs on a 20 mm square hole, cut with a 6.35 mm bit:

```text
warning: operation `op1`: tabs on `small-hole` (instance `i3`, profile 0) cover more than half of its tool path: 4 tab(s) of 14.35 mm each (tab width plus bit diameter) total 57.4 mm on a 54.6 mm path (105.1%), so the part or hole may not cut free. To let it cut free, use at most 1 tab(s) this wide, or make the 4 tab(s) at most 0.47 mm wide. The tabs are cut as set.
```

DreamCarve does not change your tabs. Export continues and cuts them as
set, because that can be what you want, for example to leave a slug in a
hole. To let the part cut free, use fewer or narrower tabs, or give the
operation that cuts small holes no tabs.

## Warnings

These still give a safe program. `check` and `export` print each one as a
sentence, after the operation's id.

| Warning starts with | Meaning |
| --- | --- |
| "tabs on ... touch or overlap" | Tabs on one outline touch or overlap, so they were joined into one longer tab. Use fewer or narrower tabs, or check that the outline is not too short for them. |
| "tabs on ... cover more than half of its tool path" | The part or hole may not cut free. See [Tabs that cover most of an outline](#tabs-that-cover-most-of-an-outline). |
| "the tab top is ... but the deepest pass stops at ..." | No pass goes below the tab top, so the tabs change nothing. The cut is shallower than the stock, or the tabs are taller than the depth reaches. |
| "the tab top is ... higher than the ... stock top" | The tab top is above the stock top. The tabs still apply: over each tab, every pass lifts clear of the stock, leaving its full thickness. |

## Removing tabs

Cut each tab with a sharp knife, a chisel, or a flush-cut saw, then sand
the stub flush. Tabs on thin or brittle material can be lower; tabs on a
heavy part can be wider or more numerous.
