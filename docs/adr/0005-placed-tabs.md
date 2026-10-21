---
status: accepted
date: 2026-10-21
---

# Tabs are placed on a part's outline and belong to the project, not to an operation

In phases 1 and 2 a tab was an operation setting: `ProfileOp::tabs` put
`count` tabs evenly, or tabs at `at_distances`, along every tool path the
Profile cut. That was quick to type in JSON when there was no GUI to place
tabs with. But it tied tabs to the cut rather than to the part (#105).
Changing the tool or the side moved every tab, so you could not keep a tab
off a detail or put one where the part is strongest. Users think of a tab
as material left at a place on the part. PureCutCNC agrees: its tabs are
project-level objects (`src/types/project.ts`, `Tab`), and any profile
that crosses one lifts over it (`src/engine/toolpaths/tabs.ts`).

A tab is now a **placed tab** in `Project::tabs`. It names an instance,
one of its definition's profiles, and a point in definition space
(`x_mm`, `y_mm`). It carries its own width, height and shape. Every
enabled Profile that cuts that outline rises over the tab, with the phase
1 span rule: the tab's width plus the tool diameter, measured along the
tool path and centred on the path's point nearest the tab.

The open questions in #105 were decided as follows.

- **A place on one instance's outline, not a rectangle in the world.**
  PureCutCNC places a rectangle, so any path that crosses it lifts over
  it, including a neighbouring part's path or a Pocket wall that happens to
  pass through. A tab here names its outline, so only the cuts of that
  outline lift over it.
- **A point, not a distance along the outline.** A distance along the
  outline was the first idea. But a tool path turns round each convex
  corner on an arc, every point of which is nearest the corner itself. A
  tab the phase 2 setting put on such an arc would then come back at the
  arc's middle: in the corpus below, 3 of 13 projects moved a tab that
  way. A point keeps every position a tool path can have, and PureCutCNC
  stores a point as well. A tab placed on the outline (`tabs place --at`,
  later the GUI) is the outline's point. A tab placed from a request is
  the request's centre on that Profile's tool path, a tool radius off the
  outline. Each is crossed by any Profile whose path passes within its
  tool radius plus half the tab's width of where the tab meets the
  outline, and whose cut goes below the tab's top.
- **Project-level, keyed by instance.** Tabs live in one list in the
  project, as PureCutCNC keeps them, rather than inside each instance.
  `tabs list` and the warnings can then see them all. The point is in
  definition space, so the tab follows its instance when the instance
  moves, rotates, mirrors or scales.
- **Size and shape per tab.** Each tab has its own `width_mm`,
  `height_mm` (tab top above the stock bottom) and `shape`, as
  PureCutCNC's do. A pass rises over a tab only when it is below that
  tab's top. Where spans of different tabs overlap they merge, as before:
  the merged span takes the highest top, and it is smooth only if every
  tab in it is.
- **The tab setting is a generator.** `TabSpec` (`count` or
  `at_distances`, width, height, shape, `on_holes`) is now a request to
  place tabs, not data to cut by. `dreamcarve tabs place <dcv> <op>
  --count N` places tabs for one operation's targets with the phase 2
  layout rules, including #109's rules for each outline: no tabs on holes
  unless `--on-holes`, and fewer tabs on an outline too short for them,
  but at least one on a part. `--at d1,d2,…` places tabs at distances
  along each outline from its first vertex. `tabs list` and `tabs clear`
  read and edit the list. `init` still writes a request for four tabs
  rather than placed tabs. Its one Profile cuts every closed shape of the
  drawing, and users then cut most of those shapes with operations of
  their own. Tabs placed at `init` would stay on all of them; the plaque
  quick start would be left with 16 such tabs.
- **Warnings.** A tab that no enabled Profile crosses warns
  `tab_not_crossed`. A tab on an outline that a Profile cuts as a hole is
  cut, because the user placed it, and warns `tab_on_hole`: the slug of a
  hole is usually waste, and holding it means work by hand. Overlap, cut
  range and coverage keep their codes. Placed tabs are never cut down at
  export. A layout that covers more than half its path warns, and its
  tabs are cut as placed.

## Format version 11 and migration

Format 11 adds `"tabs": []` to the project and changes nothing else. A
format 10 Profile's `tabs` setting stays as a *tab request* for format 11.
Placing tabs needs a Profile's tool paths, which only `dc-cam` can build,
and a `.dcv` migration step rewrites JSON only. Generation places any
request still present before it cuts, using the rules `tabs place` uses,
so an older file cuts the same with no step by the user. Each `tabs`
command that saves the project places the request of every enabled Profile
first and clears it, so the saved file holds placed tabs only. A disabled
Profile keeps its request until it is placed by name.

Placing a request reproduces the phase 2 positions exactly. The point
stored is the request's own centre on the tool path, so the nearest point
of that path is the centre again. Every golden and expected project with
tabs exports byte-identical G-code both as loaded and after `tabs place`
has saved its tabs as placed tabs. The tests cover the 13-project corpus
in `crates/dreamcarve/tests/fixtures/tabs-migration/` (both plaque
fixtures and a phase 1 grblHAL file among them), the golden cases and the
coaster programs `init` writes.

One case changes, on purpose. Two Profiles that both cut one outline with
tabs now both rise over the tabs of both, once the tabs are placed.
Before, the later cut went through the earlier one's tabs. This is safer,
and it is what a tab as material on the part means.

## Consequences

- One Profile cuts a part with tabs and its hole without: tabs are placed
  on the part only (#105's star case), whatever `on_holes` says.
- Changing a Profile's tool or side no longer moves its tabs. A Profile
  whose path runs too far from a tab does not cut round it, and if no
  Profile does, `tab_not_crossed` says so.
- The GUI (phase 4) places, drags and deletes these same objects, and
  PureCutCNC's auto-tab command (#109) writes them. Neither needs another
  format change.
- The tab request field goes away in a later format version, once
  nothing writes it. Until then, a hand-written request is still cut, and
  the next `tabs` command that saves the file places it.
