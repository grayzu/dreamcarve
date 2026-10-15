# Phase N: <name> — Functional Spec

**Status:** draft | accepted | shipped in vX.Y.Z

## Goal

One paragraph. What can a user do at the end of this phase that they could
not before?

## Users and scenarios

Who does this phase serve, and the two or three concrete things they do with
it. Write each as a short story: "Sam exports an SVG from Inkscape, …".

## Capabilities

Numbered list. Each is observable behaviour, written as "The user can …" or
"DreamCarve …". Each capability maps to at least one acceptance test below.

## Out of scope

What this phase deliberately does not do, so nobody argues it in a PR.

## Acceptance tests

Numbered, each traceable to a capability. State the input, the action, and
the observable result. These become end-to-end tests in the design spec.

## Safety requirements

Only for phases that change machine output. Each item names an invariant on
the emitted G-code and how it is checked.

## Test part

Only for phases with a real cut. Material, dimensions, tool, operation
parameters, controller, and what "success" looks like on the bench. After the
cut, record date, outcome, and any deviation here.

## Documentation deliverables

The mdBook chapters and the quick start this phase ships.

## Exit criteria

Checklist. Every item must be true before the phase tag.
