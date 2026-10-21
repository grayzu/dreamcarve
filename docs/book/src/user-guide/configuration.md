# Configuration

DreamCarve keeps a small amount of per-user configuration outside your
projects: defaults for new projects, and (from the tool library feature on)
your tool library. A project file never depends on it: everything a
project exports with is in the `.dcv`, so a project exports the same on
any machine.

## Where it lives

Run `dreamcarve config path` to see the locations on your machine and
whether each exists:

```text
Config directory: /Users/you/Library/Application Support/dreamcarve (exists)
Config file:      /Users/you/Library/Application Support/dreamcarve/config.toml (exists)
Tool library:     /Users/you/Library/Application Support/dreamcarve/tools.json (not found)
```

The directory is the operating system's usual place for an application's
settings:

| | Config directory |
| --- | --- |
| macOS | `~/Library/Application Support/dreamcarve` |
| Linux | `~/.config/dreamcarve` (or `$XDG_CONFIG_HOME/dreamcarve`) |
| Windows | `%APPDATA%\dreamcarve` (for example `C:\Users\you\AppData\Roaming\dreamcarve`) |

DreamCarve does not create the directory or the files; make them when you
want to set something.

To use another directory, set the environment variable
`DREAMCARVE_CONFIG_DIR` to it. Everything, the config file and the tool
library, is then looked up there. This is useful for a portable setup, or
for trying a setting without touching your own:

```sh
DREAMCARVE_CONFIG_DIR=/tmp/dc-try dreamcarve config path
```

## The config file

`config.toml` is a [TOML](https://toml.io) file. Every setting is optional;
a missing file, or a missing setting, means the default. The whole file
today:

```toml
[export]
tool_change_mode = "separate_programs"   # or "m6"
```

| Setting | Meaning |
| --- | --- |
| `export.tool_change_mode` | The `tool_change_mode` that `init` and `export --post` write into a new project's `post_processor` block: `"separate_programs"` (the default, one program per tool run) or `"m6"` (one program with `T<n> M6` tool changes, grblHAL only). See [Projects with more than one tool](post-processors.md#projects-with-more-than-one-tool). |

Only `init` and `export --post` read the file, when they create a new
`post_processor` block. A plain `export` or `check` works from the project
alone. A default the chosen post-processor cannot honour (`"m6"` with GRBL,
which has no `M6`) stops the command with an error naming the config file,
so a project that `check` would reject is never written.

## Mistakes in the file

A file that does not parse, and a setting DreamCarve does not know, are
errors, never silently ignored: a typo in a setting name would otherwise
turn into the default without a word. The message names the file, line
and column, in the form your editor or terminal can open:

```text
error: /Users/you/Library/Application Support/dreamcarve/config.toml:2:1: unknown field `tool_change_mod`, expected `tool_change_mode`
```
