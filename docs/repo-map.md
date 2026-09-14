# Repo map

The repo map is an **on-demand symbol map of the workspace**: a single
`repo_map(focus?)` call returns which Rust files define which functions, types,
traits, modules and macros, ranked so the part of the code this session is
working on comes first. It is the read-side use of tree-sitter (spec §9): the
official `tags.scm` is run over every `.rs` file under the session cwd.

## Where does a navigation aid go?

The repo map is one point on the same cost line as skills and `AGENTS.md`:

| Need | Where it goes |
| --- | --- |
| "What is in this repository, broadly?" | **`repo_map`** — one call, a fixed budget, ranked to the current task |
| "What does this one file / symbol do?" | **`read_file`** — you already know the path |
| "Find every place this name is used" | **grep / read** (the map lists names, not usage sites) |

The boundary rule mirrors skills: `repo_map` is for *exploring an unfamiliar
repository*, not for facts you can already name. It is deliberately **not
injected** — an injected map would have to refresh as files change, and every
refresh would push the history after it out of the cached prefix. An on-demand
tool result lands at the tail and leaves the prefix alone.

## How it works

- **Extraction.** Every `.rs` file under the cwd is parsed once with
  `tree-sitter-rust` and the grammar's official `queries/tags.scm`. The query
  yields only names and kinds — no signatures, no scope — so v1 renders names
  only. Hidden directories, `target/`, `node_modules/`, symlinks, and files over
  1 MB are skipped, and at most 2 000 files are walked, so one call cannot run
  away. The compiled query and parser are constructed once per session.
- **Cache.** Unchanged files are served from an in-memory mtime cache (aider
  keeps the same cache in SQLite; one session does not need a database). A
  repeat call re-parses only what changed, which keeps the "explore, then ask
  again with a focus" loop cheap.
- **Budget.** Fixed default **1k** estimated tokens, configurable up to a **4k**
  ceiling. There is no `tokens` argument: the budget is configuration, not a
  model decision, and a value a model sends anyway is ignored. A map that does
  not fit is cut at whole-symbol boundaries and ends with a one-line count of
  what was omitted.
- **Ranking.** A naive, inspectable pure function (spec §9), not a whole-graph
  PageRank. Session relevance first — the `focus` argument, then paths this
  session recently read or wrote, then identifiers the recent messages used —
  and a structural tiebreak (how often a name is referenced and defined). `rank`
  is the replaceable seam a weighted PageRank would drop into later, if a large
  repository ever makes it worth it.

## Measured baseline

The regression baseline is synthetic so it does not depend on what happens to be
on a machine; regenerate it with
`cargo test --release --test repo_map -- --ignored --nocapture`.

| Date | Files / bytes | Cold build | Warm (cached) build | Output |
| --- | --- | --- | --- | --- |
| 2026-09-15 | 400 / 204 800 | 92.8 ms (400 parses) | 19.0 ms | 4 093 chars ≈ 1 024 tokens |

The real-crate figure from the research ticket (`syn` + `petgraph` + `regex`,
237 files / 3.97 MB) was 395–466 ms to parse plus 226–228 ms to run the tags
query, ~133 MiB peak when every tree was retained; this map does not retain
trees, so its cost is dominated by parsing whatever changed.
