# Phase specs

One folder per phase, `phase-N/`, holding `functional.md` and `design.md`.
Copy the templates in this folder to start a phase. Both specs merge via PR
before the phase's implementation issues open. See `docs/plan.md` "How work
flows".

- **Functional spec** says what the user can do and how we know it works.
  Written for the user and the tester. No crate names, no algorithms.
- **Design spec** says how it is built and what is tested. Written for the
  implementer. Names crates, modules, types, algorithms, PureCutCNC sources,
  and the test list that becomes the issues' Tests sections.
