# User guide

DreamCarve turns a 2D drawing into a G-code program for a CNC router. In
phase 1 it is a command line tool with one operation, Profile, which cuts
along closed outlines.

## The workflow

1. Draw your part in Inkscape or any SVG editor, and save it as SVG.
2. `dreamcarve init part.svg` writes a **project file**, `part.dcv`.
3. Edit `part.dcv` in a text editor: the stock, the tool and its feeds, the
   operation, and the tabs.
4. `dreamcarve check part.dcv` reports problems without writing anything.
5. `dreamcarve export part.dcv` writes the G-code program, `part.nc`.
6. Air cut, then cut.

[Your first cut from an SVG](../quick-start/first-cut.md) walks through this
once, end to end.

## Terms

- **Stock**: the block of material you cut from.
- **Work zero**: the point on the stock the machine treats as X0 Y0 Z0. You
  set it on the machine; the project says where it is on the stock.
- **Toolpath**: the path the centre of the bit follows.
- **Feed rate**: how fast the bit moves while cutting. **Plunge rate**: how
  fast it moves straight down.
- **Safe Z**: the height above the stock at which the bit moves between
  cuts.
- **Post-processor**: the definition of the G-code dialect a controller
  reads.

Coordinates follow the machine: X to the right, Y away from you, Z up.
Every length in the project file is in millimetres.

## The commands

### `dreamcarve init <svg|dxf>`

Creates a project from an SVG or a DXF drawing, told apart by the file
extension. Each path (SVG) or shape (DXF) in the drawing becomes a shape;
closed shapes are targeted by one Profile operation. The defaults are
listed in the [quick start](../quick-start/first-cut.md#2-create-the-project).
DXF has a chapter of its own: [Importing DXF](importing-dxf.md).

| Option | Meaning |
| --- | --- |
| `-o`, `--output <DCV>` | Where to write the project. Default: the drawing's name with `.dcv`. |
| `--post <ID>` | Post-processor to use, at its latest version: `grblhal` (default) or `grbl`. |
| `--units <mm\|inch>` | Units the G-code is written in. Default `mm`. For a DXF, also the units the drawing is in: required when the file does not say, and overriding it when it does. |
| `--join-tolerance <MM>` | DXF only. Join open segments whose ends are within this distance, and close a chain whose ends meet. Default `0.01`. |
| `--layer <NAME>` | DXF only. Import only this layer; repeat the option for several. |

`init` leaves the values only you can know unset (`null`): the stock
thickness, the cut depth, and the tool's feed rate, plunge rate and spindle
speed. It then lists each one with its line and column in the file, as
`coaster.dcv:9:21: stock.thickness_mm is not set (mm)`. See
[Finding a value](project-file.md#finding-a-value).

`init` never overwrites a file: if the project already exists, it stops.
Remove the old one or choose another name with `-o`.

SVG lengths are converted to millimetres from the units the drawing
declares (`mm`, `cm`, `in`, `pt`, `pc`; plain numbers and `px` are 96 per
inch). Curves are converted to short straight lines within 0.01 mm of the
drawing. Text, images, clip paths and masks are ignored with a warning:
convert text to paths before you save.

A drawing is refused if its page is larger than 6000 mm (6 m) on a side or
any path reaches more than 6000 mm from the page's bottom-left corner.
`init` also refuses a drawing whose stock, with the 10 mm margin, would be
larger than 6000 mm on a side. See
[the project file](project-file.md#coordinates) for why the limit exists.

### `dreamcarve export <dcv>`

Generates the toolpaths and writes the program. When the enabled
operations use more than one tool, it writes one program per tool run, by
default ([Projects with more than one tool](post-processors.md#projects-with-more-than-one-tool)).

| Option | Meaning |
| --- | --- |
| `-o`, `--output <NC>` | Where to write the program. Default: the project's name with the post-processor's extension (`.nc`). With one program per tool run, its stem and extension name them: `-o job.nc` writes `job-1-T1.nc`, `job-2-T2.nc`, … |
| `--post <ID>` | Switch the project to the latest version of this post-processor, with its default settings, save the project with it, then export. With the project's own id, it upgrades to the latest version ([Versions](post-processors.md#versions)). The project is checked against the new post-processor, not the old one. |

Export writes nothing unless every step succeeds. It refuses a project with
any error, including a tool with no feed rate, plunge rate or spindle speed
(the speed is optional when the router is switched by hand), and one whose
cut would leave the stock. After the file is written it lists each
operation, prints the program's extents, names the bit to fit and set work
zero with before you start, then lists every place the program stops. The
extents are in machine coordinates, the numbers your controller shows once
work zero is set: first where the centre of the bit goes on each axis (Z
min is the deepest cut), then where its edge reaches in X and Y. Place
clamps from them ([Safety](safety.md#before-every-cut)). The coaster, with
one bit and the default automatic spindle control, does not stop:

```text
Extents (machine coordinates, mm, tool centre): X 6.825..103.175, Y 6.825..103.175, Z -12.000..5.000
  The cutter's edge reaches X 3.650..106.350, Y 3.650..106.350; keep clamps outside that.
Before you start: fit T1 Set me and set work zero with it.
```

With [manual spindle control](post-processors.md#manual-spindle-control)
the list shows the "Turn the router ON" and "Turn the router OFF"
pauses, under "Pauses and tool changes". A project with more than one bit
lists its programs in the order to run them, with what to do before each,
then each program's extents and pauses. With
[one program with M6](post-processors.md#one-program-with-m6) (grblHAL
only), the list shows each tool change instead, such as
`Fit T2 3 mm downcut. Set Z zero, then resume.`

### `dreamcarve check <dcv>`

Runs everything `export` does except writing the file, and prints every
error and warning. It also lists open paths, which Profile does not cut.
Each message about one value starts with its file, line and column, and
every value still unset is reported in the same run. When the program
could be built, it prints the program's extents as `export` does, for each
program when there is one per tool run.

### `dreamcarve posts`

Lists the bundled post-processors.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success. |
| 1 | The project has an error, a file could not be read or written, or the SVG could not be imported. |
| 2 | Bad arguments, or an unknown post-processor id. |

## Chapters

- [The project file](project-file.md): every field of `.dcv`.
- [Shapes, holes, and islands](shapes-and-holes.md): how DreamCarve decides
  what is a part and what is a hole.
- [Profile operation](profile.md): side, depth, passes, direction.
- [Tabs](tabs.md): holding the part in place.
- [Post-processors](post-processors.md): grblHAL, GRBL, and routers you
  switch on by hand.
- [Safety](safety.md): what DreamCarve checks, and what it cannot.
