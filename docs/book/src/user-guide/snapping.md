# Snapping and the grid

When you draw, move or reshape something, the pointer **snaps**: a point
near something worth hitting lands exactly on it. A marker shows what it
snapped to, with the kind's name beside it.

![The Line tool about to take its first point: the pointer near a screw hole, a marker on the hole's centre with "Centre" beside it, and X 18, Y 18 in the tool bar](figures/app/draw-snap.png)

## What a point snaps to

A point snaps when a candidate is within 14 pixels of the pointer.

| Kind | What it offers | At first |
| --- | --- | --- |
| **Endpoint** | Every corner of every visible shape, the stock's corners, the origin | On |
| **Midpoint** | The middle of every line and arc | On |
| **Centre** | The centre of every arc and circle, and of every shape that keeps its sizes | On |
| **Intersection** | Where two outlines cross, or one crosses itself | On |
| **On outline** | The nearest point of an outline | On |
| **Perpendicular** | The foot of the perpendicular from the last point to an outline | Off |
| **Grid** | The nearest point of the grid | On |

When several are in reach, an endpoint, midpoint, centre or intersection
wins, then a perpendicular, then a point on an outline, then the grid.
With nothing in reach the point is where the pointer is: a click is
never thrown away.

Shapes snap as they are drawn, so a slot's end centres, a rounded
corner's arc and the corners of a letter are all offered. A hidden shape
is not. A locked shape and
[construction geometry](groups-and-construction.md) are. A picture does
not snap.

## Turning it on and off

- The **Snapping** button, the last in the tool strip at the Viewport's
  left, is pressed while snapping is on. Click it for a list with
  **Snapping**, which turns all of it on or off, and a tick for each
  kind.
- **View ▸ Snapping** and **View ▸ Snap To** in the menu bar are the
  same switch and the same ticks. **Shift+S** turns all of it on or
  off.
- Hold **Ctrl** (**⌘** on a Mac) while you move the pointer to switch
  snapping off for as long as you hold it.

These are settings of the app, not of the project.

## Holding a direction

Hold **Shift** while you draw, and the point is held to steps of 15°
about the last point: straight along, straight up, 45° and so on. A
snap candidate on that line still wins.

## The grid

The grid's spacing follows the zoom. It is the smallest of 1, 2, 5, 10,
20, 50 mm and so on that is at least 8 pixels on screen. In an inch
project the steps are 0.01, 0.02, 0.05, 0.1 in and so on. Zoom in, and
a point snaps finer.

**View ▸ Grid ▸ Spacing** sets the spacing to a length you type instead,
until you set it back to **Automatic**. A hidden grid does not snap.

## When exact matters

Snapping is as exact as the thing snapped to. Where a size matters, type
it: every point and every size of every tool can be typed in the tool
bar, as [Drawing shapes](drawing-shapes.md#the-tool-bar) says. A typed
value does not depend on the zoom.
