# Phase 6's review pictures (slice 13)

The Simulate step as the build draws it, on the test parts. The pictures
are written by the app, through wgpu, not drawn:

```sh
cargo test -p dreamcarve-app --test review phase6_review -- --ignored
```

20 scenes, each twice (`-dark-1x.png`, `-light-1x.png`), in a 1440 × 900
window:

| Scenes | Show |
| --- | --- |
| `a1` to `a4` | The plaque finished, from Home, Top, Front and Right |
| `b1` to `b3` | The plaque part-way: inside its first operation, and as the Panel and the Square recess leave it, the rows after dimmed and the tool drawn |
| `b4` | The Panel's toolpath and rapids as lines over the stock it left |
| `c1`, `c2` | Colour by operation and by depth, the depth legend hung under Colour |
| `c3` to `c5` | Uncut material: none on the plaque as committed; the four corners of its square recess with the corner relief off, listed; and the same framed from the top by a click on the line |
| `d1`, `d2` | The sign's V-carving, from Home and from the top |
| `d3`, `d4` | The inlay pair: the stock, and the male board with its mirrored plugs |
| `e1` to `e3` | The plaque at Draft, Standard and Fine |

## What was looked at

- **The stock reads as a solid.** Tops, floors, walls and a V's slopes
  are told apart by the two lights alone, in both appearances; a floor
  is a little darker for its depth; the bed shows through the screw
  holes and the cut-out. `a1`, `d1`, `d4`.
- **Tabs stand.** The plaque's four, across the cut-out. `a1`, `c1`.
- **A V-bit's work is a slope, not a staircase.** The sign's lettering
  and the male board's plugs are lit as surfaces: the step rule classes
  their risers as slopes. `d1`, `d4`.
- **By operation colours a wall by the cut that made it**, so a through
  cut, which has no floor, still shows its operation. `c1`.
- **By depth** shades a wall down its height, and its legend is phase
  4's. `c2`.
- **The tool** is the cutter's own shape with a darker shank, and is
  not drawn at the very start or the very end. `b1`, `b2`.
- **The toolpath and the rapids** of the operation shown lie on the
  stock it left: the Panel's raster passes on its floor, the ring round
  its island, the rapids dashed above, and all of it hidden where a wall
  stands in front. At the moment between two operations it is the one
  just cut that is shown; the first build showed the next one's. `b4`.
- **Uncut material** tints the four corners and the panel lists them as
  one line; a click frames them. `c4`, `c5`.
- **The colours.** The first build drew the stock a good deal too dark:
  the target was an sRGB texture, and egui reads a texture as holding
  encoded colours. The target is plain now, and the stock is its
  token's colour under the lights.

## Walls on cell edges, or upstream's outline corners (#973)

PureCutCNC's later refinement rebuilds a wall's corners from the
outline of the cut, so that a diagonal or curved wall is smooth where
cells would give it a sawtooth. It is about 1,400 lines there and is
not ported.

At Standard (1024 cells along the longer side) the plaque's curved
walls, the sign's oval and letters and the inlay's plugs show no
sawtooth at the size a 1440 × 900 window draws them (`a1`, `d1`, `d4`).
At Draft the steps along the squiggle and the cut-out's curve can be
made out even with the whole stock in view (`e1`); at Standard and Fine
they cannot (`e2`, `e3`). Zoomed far in on a diagonal wall the cells
will show at any quality, as they do in the heightmap `simulate --out`
writes; the pictures here are all of the whole stock or of one recess.

**Decision: not ported in this phase.** Standard is the default and
reads well; Fine is one click away and costs under a second on these
parts. It stays a candidate for phase 7, where an imported model's
surfaces will be looked at more closely than a 2.5D wall is.

## What a build costs

One build of every run from uncut stock, in the optimised test profile,
on the machine the phase was built on (an Apple M-series laptop):

```sh
cargo test -p dc-job --test simulate the_cost_of_each_part -- --ignored --nocapture
```

| Part | Quality | Cells | Moves | Cells tested | Build | Design and compare |
| --- | --- | --- | --- | --- | --- | --- |
| plaque | Draft | 512 × 362 | 1,912 | 0.6 M | 4 ms | 5 ms |
| plaque | Standard | 1024 × 723 | 1,912 | 2.4 M | 13 ms | 16 ms |
| plaque | Fine | 2048 × 1446 | 1,912 | 9.6 M | 40 ms | 62 ms |
| sign | Draft | 512 × 233 | 5,452 | 0.6 M | 8 ms | 4 ms |
| sign | Standard | 1024 × 466 | 5,452 | 2.5 M | 30 ms | 17 ms |
| sign | Fine | 2048 × 931 | 5,452 | 9.9 M | 110 ms | 77 ms |
| inlay, stock | Draft | 512 × 373 | 2,949 | 1.3 M | 26 ms | 1 ms |
| inlay, stock | Standard | 1024 × 745 | 2,949 | 5.1 M | 94 ms | 6 ms |
| inlay, stock | Fine | 2048 × 1490 | 2,949 | 20.4 M | 348 ms | 22 ms |
| inlay, male board | Draft | 512 × 373 | 14,521 | 4.5 M | 41 ms | 1 ms |
| inlay, male board | Standard | 1024 × 745 | 14,521 | 18.1 M | 151 ms | 5 ms |
| inlay, male board | Fine | 2048 × 1490 | 14,521 | 72.4 M | 554 ms | 21 ms |

The work grows with the cells a move's box holds, about fourfold a
quality, and a V-bit's moves cost more a cell than an end mill's. The
slowest case here is half a second. **No optimisation is made**: the
build is serial (design question 5), and a benchmark would have nothing
to justify. A Photo V-Carve, with hundreds of thousands of moves, is
the case to measure when its fixture is simulated; it was not in this
table.

## Not checked here

- The picture on Windows and Linux: the renderer has been run on macOS
  (Metal) only. It uses no feature beyond wgpu's defaults.
- A drawn frame's time. The picture is drawn only when something it
  shows changes; at Standard a frame is about 2.2 million quads' worth
  of vertices, most of them walls of no height. It has not been timed.
- **The window driven by hand.** Everything above was seen through
  these pictures and the window's tests, which press the same buttons
  and keys. Dragging the stock round with a mouse, and how playback
  feels at each speed, are the maintainer's first look to take.
