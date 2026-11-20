# Tool library

A project carries its own copies of its tools (see
[The project file](project-file.md#tools)). The tool library is where you
keep the tools you own, set up once, so each new project can take a copy
instead of having every value typed in again.

The library belongs to you, not to a project. Copying a tool into a project
copies its values; changing or removing the tool in the library afterwards
never changes a project that already has it. A project still exports the
same on every machine, with or without your library.

## Where it lives

The library is the file `tools.json` in your config directory.
`dreamcarve config path` prints where that is and whether the file exists
yet (see [Configuration](configuration.md)):

```text
Tool library:     /Users/you/Library/Application Support/dreamcarve/tools.json (not found)
```

You do not have to create it. The first `dreamcarve tools` command that
changes the library (`tools import`) creates the file, and the config
directory if it is missing. Set `DREAMCARVE_CONFIG_DIR` to keep the library
somewhere else.

## In the app

The app keeps tools in two places, as the command line does. The
**Tools** step holds the tools of the open project, which are saved in
its file. The **Tool Library** is yours, shared by every project. The app
and the `dreamcarve tools` commands read and write the same library file.

### The Tools step

The Tools step lists the project's tools. Click one to edit it in the
Inspector; there is no Edit button.

![The Tools step with a new tool selected: three tools in the list, the new one marked with a red dot; in the Inspector Name "60 degree V-bit", Type V-bit, an empty Angle field over "Type the included angle to make this tool a V-bit", Diameter 6.35 mm, Flutes 2, Feed, Plunge and RPM outlined in red and saying Required, and Save to library at the foot](figures/app/tools-editor.png)

A tool's row has a glyph for its type, its name, and under it the
diameter and the tool number. A V-bit's row adds its angle, `6.35 mm ·
60°`. At the right:

- a **red dot** means the tool has a value still to set. Hold the pointer
  on it to see which;
- a blue **hint** mark means no operation uses the tool yet. It stops
  nothing.

| Button | Does |
| --- | --- |
| **New tool** | Adds an end mill named `tool<n>`, 6.35 mm across with 2 flutes, and nothing else set. Its name is selected, so type the name you want and press Enter |
| **Add from library…** | Opens the Tool Library, to tick tools and copy them in |
| **Duplicate** | Copies the selected tool below itself as `<name> (2)` |
| **Delete** | Deletes the selected tool. It is off while an operation uses the tool |
| **Tool Library…**, at the foot of the panel | Opens the Tool Library, as **Project ▸ Tool Library…** does |

### A tool's fields

| Field | What it is | In the project file |
| --- | --- | --- |
| **Name** | What the lists, the operations' **Tool** rows and the program's comments call the tool | `name` |
| **Type** | End mill, Ball nose, V-bit, Engraver, Drill or Tapered ball nose | `cutter` |
| **Angle** | A V-bit's **included** angle, as V-bits are sold. Only a V-bit has it | `cutter.v_bit.included_angle_deg` |
| **Tip diameter**, **Taper per side**, **Flute length** | A tapered ball nose's three numbers ([Tapered ball nose](tapered-ball-nose.md#in-the-app)). Only that type has them | `cutter.tapered_ball_nose` |
| **Diameter** | The cutting diameter, for every type | `diameter_mm` |
| **Flutes** | The number of cutting edges | `flutes` |
| **Feed** | The feed rate along a cut | `feed_mm_min` |
| **Plunge** | The plunge rate, straight down | `plunge_mm_min` |
| **RPM** | The spindle speed | `rpm` |
| **Step-down** | The most each pass cuts deeper. An operation may set its own | `step_down_mm` |
| **Stepover** | How far the bit moves over between passes that clear an area, as a percentage of its diameter | `step_over_fraction`: 40 % is 0.4 |
| **Tool number** | The bit's number on your machine, `T1`. Empty, the tool takes its place in the list | `number` |
| **Vendor**, **Product id** | For your own reference, and how the library groups and searches tools | `vendor`, `product_id` |

**Feed**, **Plunge** and **RPM** start empty and say **Required**. The
app never fills them in: a feed that is wrong for your bit and material
can break the bit or start a fire. Type the values your bit's maker
gives, or your own tested ones. Nothing can be exported while a tool an
operation uses lacks one.

Choosing **V-bit** in **Type** does not change the tool at once. The
**Angle** field appears and asks for the included angle, and the tool
becomes a V-bit when you type it. The angle decides how deep a carve
goes, so DreamCarve will not assume one. Escape leaves the tool as it
was. Every other type is taken as soon as you choose it.

Each field you leave is one step for **Undo**, *Undo Edit tool “60 degree
V-bit”*.

### Saving a tool to the library

**Save to library**, at the foot of the tool's editor, copies the project
tool into your library, and the status bar says where: *Saved “60 degree
V-bit” to the library as lt1*. Saving a tool the library already holds
changes nothing, and says *“60 degree V-bit” is already in the library as
lt1*.

The two copies are separate from then on. Editing one never changes the
other.

### The Tool Library sheet

**Tool Library…** on the Tools step, or **Project ▸ Tool Library…**,
opens the library in a sheet with two panes: the list at the left, and
the selected tool's fields at the right.

![The Tool Library after an import: "Imported 13 tools from sample.tools. 10 skipped." with Details; a search field and the type filter over New tool, Duplicate and Delete; the table's columns Name, Type, Ø, Tip, Angle, Flutes, Feed, Plunge, RPM and Product; five drills with a red dash for their feed; and the selected row, a tapered ball nose with Ø 6.35, Tip 0.762 and Angle 6.7°/side, whose fields are in the pane at the right](figures/app/library-sheet.png)

The tools are grouped by vendor, with **No vendor** last. Click a group's
header to close or open it.

| Column | Shows |
| --- | --- |
| The box at the left | Tick it to choose the tool for **Add to project** or **Delete** |
| **Name**, **Type** | The tool's name and its type |
| **Ø** | The **cutting diameter**, for every type: 6.35 for a 1/4 in tapered ball nose, never its tip |
| **Tip** | A tapered ball nose's tip diameter. Empty for every other type |
| **Angle** | A V-bit's included angle, `90°`, or a tapered ball nose's angle per side, `6.7°/side`. The two are different measures, so the column says which it holds |
| **Flutes**, **Feed**, **Plunge**, **RPM** | As in the tool's fields |
| **Product** | The product id |
| A ✓ at the right | The tool is already in the open project |

A **red dash** in a cell is a value the tool still needs: a feed, plunge
rate or RPM, or a tapered ball nose's tip or taper. It is never a
number. A library tool may lack values; the dash says what an operation
will ask for.

- **Search** matches the name, the vendor, the product id, the diameter
  and a tapered ball nose's tip: `0.762` finds the bit above. Groups with
  no match are hidden.
- The **type filter** beside it, **All types** to begin with, lists one
  type of cutter only.
- The **mm** / **in** switch in the header sets the units the sheet shows
  and takes. In inches the 6.35 mm bit reads 0.25. It does not change the
  project's units, and the library file always stores millimetres. The
  app remembers it.

### Editing a library tool

Select a row to edit that tool in the right pane. The fields are the ones
of [A tool's fields](#a-tools-fields), with the same **Required** marks. A
field is saved to the library file when you leave it.

- Library edits are not in the **Edit** menu's history. **Revert
  changes**, under the fields, puts the selected tool back the way it was
  when you opened the sheet.
- A value the library cannot take, such as a diameter of 0 or a name
  another tool already has, leaves the field marked with the reason, and
  the file is not changed.
- Editing a library tool never changes a project's copy of it.

**New tool** adds an end mill named `tool<n>`, as on the Tools step.
**Duplicate** adds a copy of the selected tool below it, named `<name>
(2)`. **Delete** removes the ticked tools, or the selected tool when none
is ticked. It names them and asks first, because a deletion from the
library cannot be undone. Projects keep their copies.

### Adding tools to a project

Tick the rows you want and click the green button at the bottom right,
which counts them: **Add 1 tool to project**. The sheet closes, and the
status bar says *Added 1 tool from the library*.

The project gets its own copy of each tool, as `dreamcarve tools add`
makes one, and the whole addition is one step for Undo. A ✓ at the right
of a row marks a tool the open project already has.

### Importing a library

**Import…** reads a Fusion 360 `.tools` or `.json` library
([Getting the file out of Fusion](#getting-the-file-out-of-fusion)), or a
DreamCarve library written by **Export library…**. Nothing is written
yet: the panes are replaced by a **review** of what the file holds.

![The import review of sample.tools: 13 New, 0 Already in your library, 0 Changed, 10 Skipped; the file's tools grouped by vendor with Type, Ø, Tip, Angle, Feed and a Status column reading New; three drills with a red dash for their feed; Back at the bottom left and Import 13 tools at the bottom right](figures/app/library-import-review.png)

The four counts are the ones `dreamcarve tools import` prints:

| Count | Means |
| --- | --- |
| **New** | Tools the library does not have |
| **Already in your library** | The same tools, imported before. They are left alone |
| **Changed** | Tools that came from this file before and now differ from yours, because the file changed or you edited yours |
| **Skipped** | Tools DreamCarve has no cutter for, each listed with its reason ([What is skipped](#what-is-skipped)) |

Each tool is listed with its **Status**. A red dash is a value the file
does not give; DreamCarve does not make one up
([Unset feeds](#unset-feeds)).

When any tool is **Changed**, the review offers a choice:

- **Keep mine**, the default, adds the file's version beside yours under
  a new name, as `tools import` does.
- **Replace with imported** overwrites yours and keeps its id, as `tools
  import --replace` does.

**Import 13 tools** writes the library and goes back to the list, with a
line saying what happened: *Imported 13 tools from sample.tools. 10
skipped.* Its **Details** shows the review again. **Back**, or Escape,
leaves the library as it was. When every tool in the file is already in
your library the button reads **Nothing to import**.

**Export library…** writes the whole library to one file. Use it to keep
a backup or to take your tools to another computer.

## Commands

| Command | What it does |
| --- | --- |
| `dreamcarve tools list` | Lists every tool: its library id, name, vendor, product id, cutter, diameter, feed, plunge rate, RPM, tool number and source. A value not set shows as `-`. |
| `dreamcarve tools list --json` | Prints the library as JSON, exactly as the file holds it. |
| `dreamcarve tools import <file>` | Adds the tools of a DreamCarve tool library file or a [Fusion 360 library](#fusion-360-tool-libraries) and reports, for each, `imported`, `imported as <new name>`, `already present`, or for a Fusion tool DreamCarve has no cutter for, `skipped` with the reason. |
| `dreamcarve tools import <file> --replace` | The same, except that a tool with the same `source` as one already in the library overwrites it (keeping its id) instead of being added beside it. |
| `dreamcarve tools export <file>` | Writes the library to `<file>`, in the same format. |
| `dreamcarve tools add <project.dcv> <tool>` | Copies a library tool into a project. |
| `dreamcarve tools remove <tool>` | Removes a tool from the library. Projects keep their copies. |

`<tool>` is a library id, such as `lt2`, or a tool's name, quoted when it
has spaces: `"1/4 end mill"`. A name works only when one tool has it; when
two do (possible in a file you edited by hand), the command lists their ids
so you can pick one.

### Listing

```text
$ dreamcarve tools list
ID   NAME                           VENDOR            PRODUCT  CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/4 end mill                   -                 -        end mill   6.35         1500         500            18000  3       -
lt2  Acme Router Bits 60° V-bit     Acme Router Bits  AC-V60   V-bit 60°  12.7         800          200            16000  -       fusion:0000-guid:preset-1
```

### Copying a tool into a project

```sh
dreamcarve tools add plaque.dcv "1/4 end mill"
```

```text
Added `1/4 end mill` (lt1) to plaque.dcv as tool t2.
```

The tool gets a new project id, the next free `t<n>` in that project, and
keeps every value from the library, including its tool `number`, which
programs then write as `T<number>` (a tool with no number is written as
its place in the project's tool list). Point an
operation at it by setting the operation's `"tool"` to that id. Library ids
(`lt<n>`) never appear in a project.

If the copied tool's number is the same as that of another tool the
project uses, `dreamcarve check` reports it; change one of the two numbers.

### Importing and exporting

`tools export` then `tools import` of the same file changes nothing: every
tool is `already present`, and the library file stays byte for byte the
same. Use this to move a library to another computer, or to share one.

A tool counts as already present when all its values match a library tool,
whatever its id or source. A tool whose name is taken by a different tool
is added under the name with its product id after it, when it has one and
no tool has that name yet, and otherwise with ` (2)` after it (then ` (3)`,
and so on), so names stay unique:

```text
$ dreamcarve tools import changed.json
`Acme Router Bits Diamond Engraver`: imported as `Acme Router Bits Diamond Engraver (AC-ED14)` (lt3)
`1/4 end mill`: imported as `1/4 end mill (2)` (lt4)
2 imported, 0 already present, 0 replaced; library: /Users/you/.../tools.json
```

`tools add` and `tools remove` take the name as stored, suffix included:
`"Acme Router Bits Diamond Engraver (AC-ED14)"`.

A file that is not a valid tool library, or that holds a tool breaking a
rule below, is refused whole, naming the tool and the value; nothing is
added.

## The file

`tools.json` is JSON, one block per tool. Each tool is written exactly as a
project writes a tool, under `"tool"`, with every value present (`null`
when not set):

```json
{
  "version": 3,
  "tools": [
    {
      "id": "lt1",
      "source": null,
      "tool": {
        "id": "lt1",
        "name": "1/4 end mill",
        "vendor": null,
        "product_id": null,
        "number": 3,
        "cutter": "end_mill",
        "diameter_mm": 6.35,
        "flutes": 2,
        "feed_mm_min": 1500.0,
        "plunge_mm_min": 500.0,
        "rpm": 18000,
        "step_down_mm": 3.0,
        "step_over_fraction": 0.45
      }
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `version` | The file format version, `3`. Version 2 added each tool's `vendor` and `product_id`, and version 3 the tapered ball nose cutter. A version 1 file is read with both `null`, and a version 1 or 2 file is saved as version 3 the next time a command changes the library. A file from a newer DreamCarve is refused rather than misread. |
| `id` | The library id, `lt<n>`. New tools get the number after the highest one in the file. |
| `source` | Where the tool came from: `"fusion:<guid>:<preset>"` for a tool imported from Fusion 360, `null` otherwise. `--replace` matches on it. |
| `tool` | The tool, with the fields described in [The project file](project-file.md#tools). Its `id` repeats the library id and is ignored when the file is read. |

You can edit the file by hand. As with a project, a length or feed may be
written with its unit (`"diameter_mm": "1/4 in"`, `"feed_mm_min": "60
in/min"`; see [Values with units](project-file.md#values-with-units)), and
the library is saved with millimetre numbers the next time a command
changes it. Inch values are rounded to the exact decimal (6 places for a
length, 3 for a feed), as Fusion import rounds them; a value in millimetres
is kept as you wrote it. A field DreamCarve does
not know, or a missing one, is an error naming where it is, never silently
ignored. Each tool must also follow these rules, which come from
PureCutCNC's tool library:

- `name` is not empty;
- `diameter_mm` is greater than 0;
- `flutes` is 1 or more;
- a V-bit's `included_angle_deg` is greater than 0 and less than 180;
- a tapered ball nose's `tip_diameter_mm`, when set, is greater than 0;
  its `taper_angle_deg`, when set, is greater than 0 and at most 45 (per
  side); its `flute_length_mm`, when set, reaches past the ball, above
  the tangent height; and its `diameter_mm` is wider than the tip
  (`tapered_ball_nose_range`; see [Tapered ball
  nose](tapered-ball-nose.md#entering-one));
- `step_over_fraction`, when set, is greater than 0 and at most 1;
- `feed_mm_min`, `plunge_mm_min`, `rpm` and `step_down_mm`, when set, are
  greater than 0, and `number`, when set, is 1 or more.

Feeds, plunge rate and RPM may be left `null` in the library, as in a
project, and so may a tapered ball nose's tip, taper and flute length.
DreamCarve never fills them in: a project using a tool without them will
not export until you set them. `tools add` says which values a tool it
copies still needs.

## V-bit angle and cone height

A V-bit's `included_angle_deg` is the **included** angle, the full angle
between its two cutting edges, as V-bits are sold: a "90° V-bit" is
`{ "v_bit": { "included_angle_deg": 90.0 } }`. Fusion describes the same
bit by its taper per side, half that (see
[How Fusion tools map](#how-fusion-tools-map)).

Its `diameter_mm` is the widest the cone gets, usually the shank's. The
angle and the diameter together set the bit's **cone height**, the depth
at which its sides reach that diameter:

```text
cone height = diameter_mm / (2 × tan(included_angle_deg / 2))
```

A 6.35 mm bit has a cone height of 24.1 mm at 15°, 11.8 mm at 30°, 5.5 mm
at 60° and 3.175 mm at 90°. A V-Carve, Inlay, Photo V-Carve or countersink
that would carve deeper than that is refused with `v_bit_cone_exceeded`,
because the shank would cut, so set the diameter to the bit's real
cutting diameter, not a rounder number. [Choosing a bit
angle](v-carve.md#choosing-a-bit-angle) says what each angle is good for.

## Tapered ball nose

A tapered ball nose is entered with four numbers: `tip_diameter_mm`, the
ball's diameter (twice the tip radius vendors usually give);
`taper_angle_deg`, the taper **per side**, half the included angle, as
tapered bits are sold (so not a V-bit's convention: do not double it);
`flute_length_mm`; and `diameter_mm`, the cutting diameter, where the
taper becomes a cylinder:

```json
"cutter": { "tapered_ball_nose": { "tip_diameter_mm": 0.762, "taper_angle_deg": 6.7, "flute_length_mm": 19.05 } },
"diameter_mm": 6.35,
```

Its **cutting height**, the deepest its flank may be engaged, takes the
place of a V-bit's cone height: the lower of the flute length and the
height where the taper reaches the cutting diameter. A carve or inlay
deeper than that is refused with `v_bit_cone_exceeded`, naming which.
`tools list` prints a line of the bit's numbers after the table, `-` for
one not set:

```text
lt2 0.762 mm tip tapered ball nose: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes
```

[Tapered ball nose](tapered-ball-nose.md) says what each number does,
how to measure the taper with two plunge holes, and how the bit carves
and inlays.

## Fusion 360 tool libraries

`tools import` also reads a Fusion 360 tool library, so the tools you set
up in Fusion, or a library a tool vendor publishes for Fusion, need not be
typed in again. Autodesk publishes this format, and DreamCarve reads it the
way Autodesk describes it.

### Getting the file out of Fusion

In Fusion's **Manufacture** workspace, open **Manage → Tool Library**,
right-click the library in the list on the left and choose **Export**.
Fusion writes a `.tools` file (a zip holding the library as JSON). Vendor
libraries for Fusion often come as `.json`; that is the same library,
unzipped. Either works:

```sh
dreamcarve tools import "My Router Bits.tools"
```

DreamCarve tells a Fusion library from a DreamCarve one by its content,
not its name.

### What the import reports

Each tool gets one line: `imported`, `already present`, or `skipped` with
the reason. A value the library did not set is noted on the tool's line.
Then come the counts:

```text
$ dreamcarve tools import sample.tools
`DreamCarve fixtures 6 mm Flat End Mill`: imported (lt1)
`12 mm 90-Degree Chamfer Mill`: imported (lt2)
`5 mm Drill`: imported (lt3); feed unset
`3.175 mm Flat End Mill (Plywood)`: imported (lt4)
`3.175 mm Flat End Mill (Acrylic)`: imported (lt5)
`6 mm Thread Mill, 1 mm Pitch`: skipped: unsupported tool type `thread mill`
5 imported, 0 already present, 0 replaced, 1 skipped; library: /Users/you/.../tools.json
```

Each imported tool's `source` is `fusion:<tool guid>:<preset guid>`, so
importing the same library again reports every tool as `already present`
and changes nothing. After you change a tool's presets in Fusion, import
with `--replace` to overwrite the tools it made before instead of adding
new ones beside them.

### How Fusion tools map

| Fusion tool type | DreamCarve cutter | Notes |
| --- | --- | --- |
| flat end mill, slot mill | end mill | |
| bull nose end mill | end mill | DreamCarve has no corner radius; the name gains ` (bull nose r<RE>)` so you can tell it apart. |
| ball end mill | ball nose | |
| chamfer mill | V-bit | Included angle is twice Fusion's taper angle `TA`: a 90° chamfer mill has `TA` 45 in Fusion and imports as a 90° V-bit. |
| tapered mill with a ball tip | tapered ball nose | Only when the corner radius `RE` is half of `DC`; see [Tapered mills](#tapered-mills). A flat or bull tip is skipped. |
| drill, spot drill, center drill, counter sink, counter bore, reamer | drill | |

| Fusion value | DreamCarve value |
| --- | --- |
| `description` | `name`, after the vendor (a tool without one takes its product id, else `<diameter> <unit> <type>`) |
| `vendor` | `vendor`, and the start of the name: `IDC Woodcraft 1/4" Downcut`. A description that already starts with the vendor is not given it twice. |
| `product-id` | `product_id`; also added to the name, as `(<product id>)`, when another tool already has the name |
| `geometry.DC` (cutting diameter) | `diameter_mm` |
| `geometry.NOF` (flutes) | `flutes`; 0 or missing becomes 1, noted |
| `post-process.number` (tool number) | `number` when above 0; 0 means not set |
| preset `n` (spindle speed) | `rpm` |
| preset `v_f` (cutting feed) | `feed_mm_min` |
| preset `v_f_plunge` (plunge feed) | `plunge_mm_min` |
| preset `stepdown` | `step_down_mm`, unless the preset's **use stepdown** is off |
| preset `stepover` | `step_over_fraction`, the stepover divided by the diameter (at most 1, noted when Fusion's is wider), unless **use stepover** is off |

Lengths and feeds in an inch library are converted to millimetres and
mm/min, then rounded to 6 decimal places for a length and 3 for a feed, so
3/16" is 4.7625 mm and 60 in/min is 1524 mm/min exactly; spindle speed has
no unit. The stepover fraction is rounded to 6 places too. A millimetre
library's values are kept as written. Values Fusion keeps that DreamCarve does
not use (holder, lengths, coolant, materials, cutting speed, feed
per tooth) are ignored, and so is the shaft diameter of every tool but a
tapered mill.

### Tapered mills

A Fusion `tapered mill` whose corner radius `RE` is half its diameter
`DC` (within 0.000001 in the file's unit) has a ball tip, and imports as
a tapered ball nose:

| Fusion value | DreamCarve value |
| --- | --- |
| `geometry.DC` (the tip's diameter) | `tip_diameter_mm`; twice `RE` when the file has no `DC` |
| `geometry.TA` (taper angle) | `taper_angle_deg`, as written: Fusion's taper angle is per side, half the included angle |
| `geometry.LCF` (flute length) | `flute_length_mm` |
| `geometry.SFDM` (shaft diameter) | `diameter_mm`, the cutting diameter, where the taper reaches the shank's width |

The other values (name, flutes, number, presets and feeds) map as for
every Fusion tool; the stepover fraction is of the shaft diameter.

A value the file lacks, `TA`, `LCF` or the tip (neither `DC` nor `RE`),
is imported unset, never guessed, and the import line names each field
to set:

```text
`IDC Ultra-High Detail`: imported (lt3); set cutter.tapered_ball_nose.taper_angle_deg, cutter.tapered_ball_nose.flute_length_mm
```

Set them in the library or in the project before carving: `dreamcarve
check` lists each unset value of a tool an operation uses, and `export`
refuses until it is set.

The shaft diameter is the exception. It is the cutting diameter, which
every tool must have, so a ball-tipped tapered mill without `SFDM` is
skipped, saying `a tapered ball nose needs its shaft diameter (SFDM), the
cutting diameter`. Add the shaft diameter in Fusion and import again.

A tapered mill with a flat tip (`RE` 0 or missing) or a bull tip (`RE`
below half of `DC`) is a V-bit with a flat, which DreamCarve does not
model yet; it is skipped, saying so. A value that is there but out of
range, such as a `TA` of 50 or an `RE` above half of `DC`, skips the tool
as invalid, naming the field.

**Presets.** Each Fusion cutting preset becomes one library tool, with the
preset's feeds, so a tool with presets for plywood and acrylic becomes two
tools: `<description> (Plywood)` and `<description> (Acrylic)`. A tool
with one preset keeps its description as the name. A tool with no presets
imports once with no feeds. All the tools made from one Fusion tool share
its tool number; that is fine in the library, and `dreamcarve check`
reports it only if one project uses two of them.

### Vendors and product ids

Each tool keeps the vendor and product id the library gives it, and its
name starts with the vendor, so two vendors' `1/4" Downcut` bits stay
apart in `tools list` and in `tools add`:

```text
$ dreamcarve tools import IDCWoodcraftFusion360Library.json
`IDC Woodcraft 1/8" Acrylic O Flute`: imported (lt1)
…
`IDC Woodcraft Widget Works Diamond Engraver`: imported (lt14)
…
`IDC Woodcraft Widget Works Diamond Engraver`: imported as `IDC Woodcraft Widget Works Diamond Engraver (ED-14)` (lt49)
```

A library without vendors imports with the names it always had.

### Libraries imported by an earlier build

Tools imported by an earlier build keep the names and values they were
saved with: DreamCarve does not round a library or project when it opens
it, since it cannot tell a value you typed in millimetres from one
converted from inches. Import the Fusion library again with `--replace`
to bring them up to date: each tool is overwritten in place, keeping its
library id, with the vendor name and rounded values. Without `--replace`,
the import adds the new tools beside the old ones. Projects keep the
copies they already have.

### Unset feeds

DreamCarve never invents a feed, plunge rate or spindle speed. A preset
without one imports with that value unset (`-` in `tools list`, `null` in
the file), and the import line says which. Fusion drill presets usually
have a plunge feed and no cutting feed: that drill imports with
`feed unset`, which a Drill operation never needs. A project that uses a
tool for an operation needing the missing value will not export until you
set it in the project, or in the library before you copy the tool.

### What is skipped

DreamCarve has no cutter for these, so they are skipped, each by its
Fusion type: radius, lollipop, dovetail, face, form, thread,
corner chamfer and circle segment mills, and tapered mills without a
ball tip ([Tapered mills](#tapered-mills)); taps and boring bars; turning
tools; plasma, laser and waterjet cutters; additive (depositing) tools;
probes; and holders. A tool whose type Fusion's schema does not name is
skipped as `unknown tool type`. A tool without a cutting diameter, or with
a unit other than millimetres or inches, is skipped too, rather than
guessed at.

A file that is not a Fusion library at all, a damaged `.tools` archive, or
a library older than Fusion's schema version 11 is refused whole, and the
library is left as it was.
