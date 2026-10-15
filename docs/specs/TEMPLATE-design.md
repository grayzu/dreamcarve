# Phase N: <name> — Design Spec

**Status:** draft | accepted | shipped in vX.Y.Z
**Functional spec:** `functional.md`

## Overview

How this phase's capabilities are realised, in a few paragraphs. Name the
crates touched and the main data flow.

## PureCutCNC sources

Table: DreamCarve module → PureCutCNC file(s) it ports or adapts → notes on
what changes in the port (coordinate convention, naming, dropped behaviour).
Files listed here need the upstream attribution header.

## Data model

New or changed types, as Rust signatures with doc comments. Serialization
changes and the migration they need.

## Algorithms

One subsection per non-trivial algorithm: inputs, outputs, steps, edge cases,
and the coordinate and unit conventions it assumes.

## Module layout

Where new code lives in the workspace, and what each module's public surface
is.

## Test plan

The test list, grouped by slice. Each test has a name and a one-line
assertion. This section is copied into the GitHub issues' Tests sections.
Mark ported PureCutCNC tests with their source path. Mark golden-corpus tests
with their fixture name.

## Slices

Ordered list of implementation issues, each with the tests it owns and its
dependencies. Small enough for one PR each.

## Open questions

Anything unresolved. Empty before the spec is accepted.
