---
status: accepted
date: 2026-10-14
---

# DreamCarve has its own project format and does not read or write `.camj` natively

PureCutCNC projects are `.camj` JSON files, versioned (3.1 at the time of
writing), Y-down, with a data model shaped by its migration history. We
could have adopted them for interoperability. Instead DreamCarve projects are
`.dcv` JSON files with their own schema, a `version` field, and in-memory
migrations tested from day one.

Byte-level `.camj` compatibility would tie every schema decision to a
project we do not control, force Y-down into our model (see ADR-0001), and
make our migrations track theirs. A `.camj` *importer* is a planned phase 8
feature and also serves the parity corpus; that gives users the migration
path without the coupling.

## Consequences

- Projects embed copies of the tools and the post-processor they use, so a
  saved `.dcv` produces the same G-code regardless of library edits or app
  upgrades. Replacing the embedded copy is an explicit user action.
- Bumping the schema means bumping `version`, adding a migration, and adding
  a fixture file of the previous version to the migration tests, all in the
  same change.
