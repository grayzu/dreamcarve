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
| `height_mm` | Height of the tab top above the stock bottom: how thick the bridge is. |
| `shape` | `"rect"` (square shoulders) or `"smooth"` (a gentle ramp up and down). |

The coaster's tabs are 8 mm wide and 3 mm tall, in 12 mm stock: each pass
deeper than 3 mm above the stock bottom rises to Z 3 mm over each tab. With
work zero at the stock top, that is Z −9 in the program.

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
- **Smooth.** The bit rises and falls along the path in a smooth curve,
  reaching the tab top at the middle of the stretch. It never stops moving
  along the path, which leaves fewer marks. Every move is at the feed rate.

Passes at or above the tab top are not changed. Only the passes that cut
below it rise over the tabs.

## Warnings

| Warning | Meaning |
| --- | --- |
| `tabs_overlap` | Tabs on one outline touch or overlap, so they were joined into one longer tab. Use fewer or narrower tabs, or check that the outline is not too short for them. |
| `tabs_outside_cut_range`, `reason=cut_stops_above_tab` | No pass goes below the tab top, so the tabs change nothing. The cut is shallower than the stock, or the tabs are taller than the depth reaches. |
| `tabs_outside_cut_range`, `reason=above_stock_top` | The tab top is above the stock top. The tabs still apply: over each tab, every pass lifts clear of the stock, leaving its full thickness. |

## Removing tabs

Cut each tab with a sharp knife, a chisel, or a flush-cut saw, then sand
the stub flush. Tabs on thin or brittle material can be lower; tabs on a
heavy part can be wider or more numerous.
