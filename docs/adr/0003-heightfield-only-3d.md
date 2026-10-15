---
status: accepted
date: 2026-10-14
---

# The 3D view is a heightfield simulation; there is no CSG model preview

PureCutCNC has two 3D systems: a CSG model of the intended part (Manifold
booleans) and a heightfield material-removal simulation. DreamCarve builds
only the heightfield. The simulated result of the generated toolpaths is the
3D view.

For 2.5D work the simulation shows what the cutter will actually leave,
including scallops and unreachable corners, which is what a hobbyist needs to
check before cutting. One system is cheaper to build and test than two, and
the Rust CSG ecosystem at decision time was either archived, very young, or a
C++ build dependency. The cost is that the 3D view is empty until toolpaths
exist, and that a heightfield cannot represent undercuts, which 3-axis 2.5D
work never produces.

## Consequences

- Phase 7 (mesh machining) must re-examine this decision: retained-material
  math around an imported model may need true solid booleans. If so, add a
  CSG crate then and amend this ADR rather than working around it.
- Simulation resolution is a user-visible setting with a memory budget, since
  fine V-carve detail on a large sheet needs a dense grid.
