<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Inlay programs before the one-file Inlay (#498)

The programs the build before #498 (`main` at 4695261) exported from the
inlay pair projects of `../../fixtures/inlay-migration/`, with
`--date 2026-10-15`:

| Project | Format | Programs then | Here |
| --- | --- | --- | --- |
| `dc-male.dcv` | 24 | `dc-male-1-T2.nc`, `-2-T1`, `-3-T2` | `dc-male-male-1-T2.nc`, `-2-T1`, `-3-T2` |
| `dc-female.dcv` | 24 | `dc-female-1-T2.nc`, `-2-T1` | `dc-female-female-1-T2.nc`, `-2-T1` |
| `star-male.dcv` | 23 | `star-male-1-T2.nc`, `-2-T1`, `-3-T2` | `star-male-male-1-T2.nc`, `-2-T1`, `-3-T2` |
| `star-female.dcv` | 23 | `star-female-1-T2.nc`, `-2-T1` | `star-female-female-1-T2.nc`, `-2-T1` |

Each file is renamed as `export` names it since #498 (`<stem>-female-<k>-T<n>`
and `<stem>-male-<k>-T<n>`); the bytes are unchanged. The `dc-*` projects
are the tutorial pair as `tests/fixtures/` held them before #498, and the
star pair is the inlay test part #446 replaced, from `tests/fixtures/` at
1f1984c's parent.

`cli::v24_male_project_exports_the_same_male_programs` and
`cli::v24_female_project_exports_the_same_female_programs` hold the
migrated files to these programs, and `cli::export::programs_are_grouped_by_board`
holds the one-file tutorial project `tests/fixtures/dc.dcv` to the `dc`
ones: one project cuts what the pair did.
