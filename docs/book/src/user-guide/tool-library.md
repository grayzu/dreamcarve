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
| `dreamcarve tools import <file>` | Adds the tools of a DreamCarve tool library file and reports, for each, `imported`, `imported as <new name>`, or `already present`. |
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
keeps every value from the library, including its tool `number`. Point an
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

## Fusion 360 tool libraries (coming)

Importing a Fusion 360 tool library (`.tools` or `.json`) lands in a later
phase 2 release. Today `tools import` recognises such a file and stops with
`Fusion import lands in slice 21`, without changing the library.
