# Triage Labels

The skills speak in terms of five canonical triage roles. This file maps those roles to the actual label strings used in this repo's issue tracker.

| Label in mattpocock/skills | Label in our tracker | Meaning                                  |
| -------------------------- | -------------------- | ---------------------------------------- |
| `needs-triage`             | `needs-triage`       | Maintainer needs to evaluate this issue  |
| `needs-info`               | `needs-info`         | Waiting on reporter for more information |
| `ready-for-agent`          | `ready-for-agent`    | Fully specified, ready for an AFK agent  |
| `ready-for-human`          | `ready-for-human`    | Requires human implementation            |
| `wontfix`                  | `wontfix`            | Will not be actioned                     |

When a skill mentions a role (e.g. "apply the AFK-ready triage label"), use the corresponding label string from this table.

Edit the right-hand column to match whatever vocabulary you actually use.

## Phase labels

Every issue also carries one `phase-N` label for the roadmap phase in
`docs/plan.md` where it is planned to be resolved, so the tracker can be
filtered by phase (`gh issue list --label phase-2`).

- Apply it when triaging, alongside the triage label. A closed issue keeps the
  label of the phase it landed in.
- Pick the phase from the issue itself first ("Must land before `v0.1.0`"),
  then from where `docs/plan.md` puts the feature. Work that fixes a released
  phase's behaviour belongs to the current phase unless it is deferred.
- A tracking issue that spans phases takes the earliest phase its items are
  planned for; each spun-out issue gets its own label.
- Create a phase's label when its first issue is filed:
  `gh label create phase-N --color 5319e7 --description "Planned for phase N: <goal> (v0.N.0)"`.
