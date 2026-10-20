---
status: accepted
date: 2026-10-19
---

# Projects reference a versioned bundled post-processor and store only the user's settings

ADR-0002 had every `.dcv` embed a full copy of its post-processor so that an
upgrade never changes a saved project's G-code. That copy is about 45
fields of controller dialect, of which a user may change five. The dialect
fields are verified against controller source; a copy edited in a project
loses that (`"rapid_command": "G1"` turns every rapid into a feed move), and
an old copy drifts silently from the verified definition (#106).

A project's `post_processor` now holds the `id` and `definition_version` of
a bundled definition plus the user's settings (`file_extension`,
`spindle_control`, `spindle_on_dwell_s`, `tool_change_mode`,
`decimal_places`). Definitions are compiled in as
`definitions/<id>/v<n>.json`. A change that changes emitted bytes is a new
version file; released versions stay in the binary unchanged, pinned by
hash in a test. This keeps ADR-0002's guarantee by a different means.

## Consequences

- Supersedes ADR-0002's "embed copies of … the post-processor". Tools stay
  embedded.
- `init` and `export --post <id>` write the latest version with its default
  settings. `export --post` on the project's own id is how a user upgrades:
  an explicit action, reported as `grblhal v1 → v2`.
- An unknown `id` or `definition_version` fails to load, naming what is
  bundled. Format version 7's migration replaces a version 6 copy by a
  reference only when its dialect equals the bundled version 1; otherwise
  loading fails with every difference, never a silent swap.
- Definitions for other controllers (LinuxCNC, Mach3, …) come later as
  separate definition files, never edited inside a project.
