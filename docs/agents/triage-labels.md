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

## Colours

[`label-colors.json`](label-colors.json) is the label-to-colour map for the remote tracker, in GitHub's `rrggbb` form. It also carries the five `wayfinder:*` labels the wayfinder skill files its tickets under, so the whole vocabulary is created in one pass:

```sh
gh label create "ready-for-agent" --color "$(jq -r '."ready-for-agent"' docs/agents/label-colors.json)"
```

Nothing reads this file at runtime — it is the record of what the labels look like, so a rename here and a rename in the table above happen in the same change instead of drifting apart.
