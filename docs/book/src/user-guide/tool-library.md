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

## Commands

| Command | What it does |
| --- | --- |
| `dreamcarve tools list` | Lists every tool: its library id, name, cutter, diameter, feed, plunge rate, RPM, tool number and source. A value not set shows as `-`. |
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
ID   NAME          CUTTER     DIAMETER_MM  FEED_MM_MIN  PLUNGE_MM_MIN  RPM    NUMBER  SOURCE
lt1  1/4 end mill  end mill   6.35         1500         500            18000  3       -
lt2  60° V-bit     V-bit 60°  12.7         800          200            16000  -       fusion:0000-guid:preset-1
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
is added under the name with ` (2)` after it (then ` (3)`, and so on), so
names stay unique:

```text
$ dreamcarve tools import changed.json
`60° V-bit`: imported as `60° V-bit (2)` (lt3)
1 imported, 0 already present, 0 replaced; library: /Users/you/.../tools.json
```

A file that is not a valid tool library, or that holds a tool breaking a
rule below, is refused whole, naming the tool and the value; nothing is
added.

## The file

`tools.json` is JSON, one block per tool. Each tool is written exactly as a
project writes a tool, under `"tool"`, with every value present (`null`
when not set):

```json
{
  "version": 1,
  "tools": [
    {
      "id": "lt1",
      "source": null,
      "tool": {
        "id": "lt1",
        "name": "1/4 end mill",
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
| `version` | The file format version, `1`. A file from a newer DreamCarve is refused rather than misread. |
| `id` | The library id, `lt<n>`. New tools get the number after the highest one in the file. |
| `source` | Where the tool came from: `"fusion:<guid>:<preset>"` for a tool imported from Fusion 360, `null` otherwise. `--replace` matches on it. |
| `tool` | The tool, with the fields described in [The project file](project-file.md#tools). Its `id` repeats the library id and is ignored when the file is read. |

You can edit the file by hand. As with a project, a field DreamCarve does
not know, or a missing one, is an error naming where it is, never silently
ignored. Each tool must also follow these rules, which come from
PureCutCNC's tool library:

- `name` is not empty;
- `diameter_mm` is greater than 0;
- `flutes` is 1 or more;
- a V-bit's `included_angle_deg` is greater than 0 and less than 180;
- `step_over_fraction`, when set, is greater than 0 and at most 1;
- `feed_mm_min`, `plunge_mm_min`, `rpm` and `step_down_mm`, when set, are
  greater than 0, and `number`, when set, is 1 or more.

Feeds, plunge rate and RPM may be left `null` in the library, as in a
project. DreamCarve never fills them in: a project using a tool without
them will not export until you set them.

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
`6 mm Flat End Mill`: imported (lt1)
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
| drill, spot drill, center drill, counter sink, counter bore, reamer | drill | |

| Fusion value | DreamCarve value |
| --- | --- |
| `description` | `name` (a tool without one takes its product id, else `<diameter> <unit> <type>`) |
| `geometry.DC` (cutting diameter) | `diameter_mm` |
| `geometry.NOF` (flutes) | `flutes`; 0 or missing becomes 1, noted |
| `post-process.number` (tool number) | `number` when above 0; 0 means not set |
| preset `n` (spindle speed) | `rpm` |
| preset `v_f` (cutting feed) | `feed_mm_min` |
| preset `v_f_plunge` (plunge feed) | `plunge_mm_min` |
| preset `stepdown` | `step_down_mm`, unless the preset's **use stepdown** is off |
| preset `stepover` | `step_over_fraction`, the stepover divided by the diameter (at most 1, noted when Fusion's is wider), unless **use stepover** is off |

Lengths and feeds in an inch library are converted to millimetres and
mm/min; spindle speed has no unit. Values Fusion keeps that DreamCarve does
not use (holder, shaft, lengths, coolant, materials, cutting speed, feed
per tooth) are ignored.

**Presets.** Each Fusion cutting preset becomes one library tool, with the
preset's feeds, so a tool with presets for plywood and acrylic becomes two
tools: `<description> (Plywood)` and `<description> (Acrylic)`. A tool
with one preset keeps its description as the name. A tool with no presets
imports once with no feeds. All the tools made from one Fusion tool share
its tool number; that is fine in the library, and `dreamcarve check`
reports it only if one project uses two of them.

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
Fusion type: radius, lollipop, dovetail, face, form, thread, tapered,
corner chamfer and circle segment mills; taps and boring bars; turning
tools; plasma, laser and waterjet cutters; additive (depositing) tools;
probes; and holders. A tool whose type Fusion's schema does not name is
skipped as `unknown tool type`. A tool without a cutting diameter, or with
a unit other than millimetres or inches, is skipped too, rather than
guessed at.

A file that is not a Fusion library at all, a damaged `.tools` archive, or
a library older than Fusion's schema version 11 is refused whole, and the
library is left as it was.
