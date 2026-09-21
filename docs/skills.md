# Skills

A skill is a **progressively disclosed instruction pack**: its description is
present every turn (cheap), its full text is loaded only when it is actually
needed (expensive, but paid once). This document is about *where an instruction
belongs*; the mechanism itself is in spec §9.

## Where does an instruction go?

Ask what the instruction costs if it is present every turn, and whether the
harness must guarantee it:

| Content | Where it goes | Test |
| --- | --- | --- |
| Project facts — build/test commands, code style, directory conventions, hard constraints | **`AGENTS.md`** | "Is this in effect in **more than 80%** of turns?" |
| Task-scoped or long instructions — a release process, a migration runbook, a class of refactor, steps for an external system, long reference material | **skill** | "Is it only relevant to specific tasks, or too long to pay for every turn?" |
| Something that must happen unconditionally — formatting, running tests, refusing a class of command | **hook** | "Does it need the harness to *guarantee* it, rather than trusting the model to comply?" |

The point of the split is cost, not capability. `AGENTS.md` is injected in full
every turn, so it gets more expensive as it grows. A skill pays only when the
model decides the description matches the task. A hook is the only one of the
three the model cannot ignore.

`AGENTS.md` is never a place for long, occasionally-relevant instructions, and a
skill is never a place for something that must be enforced: the model may simply
not call the skill.

## How it works

- **Catalog.** At assembly, every `SKILL.md` under the discovery roots is read
  for its `name` and `description`. The catalog of `name: description` lines is
  recorded as a `ContextInjected { source: SkillsCatalog }` event and projected
  into the pinned head of the context: it **shares the first `user` message with
  `AGENTS.md`** (so the wire never carries two consecutive same-role messages),
  and that message is never trimmed and is byte-stable across turns so the prefix
  cache keeps hitting. The catalog is capped at 3k estimated tokens; late entries
  are omitted whole with a count.
- **Loading.** The built-in `skill(name)` tool returns the body (frontmatter
  stripped) as a normal tool result, appended at the tail, so the cached prefix
  never moves. The result is a normal tool result: it is accounted for,
  truncated, and covered by the permission language (a `Tool("skill")` rule can
  deny it).
- **Discovery.** Project level before user level, three roots each, most specific
  first; the first root to define a name wins:

  | Level | Roots (left beats right) |
  | --- | --- |
  | project | `<repo>/.fs-agent/skills/` → `<repo>/.agents/skills/` → `<repo>/.claude/skills/` |
  | user | `~/.config/fs-agent/skills/` → `~/.agents/skills/` → `~/.claude/skills/` |

  Following the `.agents` and `.claude` conventions means an existing library
  works without moving anything.
- **`disable-model-invocation: true`.** A skill with this flag is neither in the
  catalog nor loadable by `skill(name)`: the model cannot guess its name around
  the flag. Only the user invokes it.
- **User invocation.** `/<name> [task]` in the interactive session loads a skill
  the user names — including a `disable-model-invocation: true` one — and then
  runs `task` as an ordinary turn (a bare `/<name>` runs a default prompt). The
  body is a `ContextInjected { source: Skill }` appended **after** the history,
  exactly like a tool-loaded body, so the cached prefix never moves.
- **Budgets.** One body is capped at 5k estimated tokens (truncated with a
  pointer to the file, never refused — though a user-level skill's file sits
  outside the workspace, where `read_file` cannot reach it); the loaded bodies in
  one request are capped at 25k, with the oldest dropped first and the active
  turn included; the catalog is capped at 3k on its own. The 25k cap is enforced
  by `context::trim`, independently of the window budget.
- **Skills carry instructions only.** v1 does not package tools into a skill:
  the tool table is part of the request prefix and is fixed at assembly, so
  adding a tool on skill load would throw the prefix cache away.

## File format

```
<name>/SKILL.md
```

```markdown
---
name: release
description: Cut a release: version bump, changelog, tag, and publish. Use when the user asks to release or ship.
---

Step-by-step instructions...
```

`name` defaults to the directory name when omitted. A `SKILL.md` without a
`description` is skipped: the description is what the catalog is made of.
