# ADR-0003 — `status` stays a hardcoded enum, gains `blocked`

- **Status:** Accepted
- **Date:** 2026-07-26
- **Deciders:** Bastien Gallay (maintainer)
- **Supersedes:** —
- **Relates to:** [ADR-0002](0002-partial-schema-adoption.md); issue #37

## Context

Since F-schema-v2, every other taxonomy field — `type`, `class`, `effort`,
`area`, `horizon`, `severity` — is config-declared: a project lists its own
closed set of values in `.roadmap/config.toml` `[fields.*]`, and `validate`
enforces membership. `status` never joined that move; it stays a hardcoded
Rust enum (`Status::{Wip, Todo, Done}`).

Issue #37 surfaced a real gap: there was no way to express **blocked**
— work that is scoped and wanted but cannot start for a reason outside the
project (termherd's `F-fork-detection` waits on Claude Code reintroducing
forked session files). Today that state lives as bold prose inside the
feature body, invisible to sort and to the catalog. Two shapes were on the
table:

- **A.** Add `Blocked` to the hardcoded enum.
- **B.** Make `status` config-declared like every other field:

  ```toml
  [fields.status]
  values = ["wip", "blocked", "todo", "done"]
  glyphs = { wip = "🚧", blocked = "⛔", todo = "☐", done = "✅" }
  ```

B is the shape the rest of the schema already took, and it would retire
the last hardcoded taxonomy — worth deciding on deliberately rather than
defaulting into A by inertia.

## Decision

**Ship A: `status` stays a hardcoded enum, gaining a fourth value.**

```rust
pub enum Status { Wip, Blocked, Todo, Done }   // 🚧 / ⛔ / ☐ / ✅
```

`Blocked` ranks between `Wip` and `Todo` in `Status::rank()`: a blocked
item is closer to in-flight than untouched work, and it wants to be seen
rather than picked up. The change is purely additive — existing files
that never used `blocked` are unaffected, and `#[serde(rename_all =
"lowercase")]` gives it the frontmatter value `"blocked"` for free.

### Why not B

Every other config-declared field is an opaque label: the generator reads
it, displays it, and orders it by declaration position, but attaches no
further meaning to any individual value. `status` is not that. Two other
things are keyed off specific `Status` values, not off "whatever this
project calls its states":

- `Status::rank()` fixes the catalog's row order. A config-declared field
  gets its rank from declaration order in `[fields.*]`, which works fine
  for `horizon` (any order is a legitimate project choice) but `status`'s
  order carries meaning roadmark itself relies on — in-flight work sorts
  near the top regardless of what a project calls it.
- `Shipped` and `shipped_order` — the flip-time metadata that makes the
  shipped tier's order survive regeneration — are keyed off `Done`
  specifically (`render`'s `shipped_line`, `sort_key`'s tiebreak). Making
  `status` an arbitrary declared value would still require a distinguished
  done-ness predicate underneath it: some way to say "this value is the
  one that means done" that a plain `values = [...]` list does not carry.

So generalising `status` the way `horizon` was generalised doesn't remove
that coupling, it just relocates it — from a Rust enum into a second,
harder-to-see place (a config key, or a magic value, meaning "this is the
`Done` one"). That's a real cost, and it buys reuse for exactly one value
(`blocked`) that a four-line enum change already delivers. Not worth
paying yet.

## Consequences

### Positive

- Cheapest fix for the concrete problem: `blocked` items stop reading as
  `todo`, sort visibly between in-flight and untouched work, and get their
  own glyph.
- No config or `validate` changes: `status` keeps working exactly as
  before, everywhere it already appears in code and docs, plus the new
  value.

### Negative / accepted trade-offs

- `status` remains the one taxonomy axis a project cannot rename, reorder,
  or extend without a roadmark code change. A project that wants a status
  value roadmark doesn't ship (e.g. `review`, `cancelled`) has to fork or
  file an issue, unlike every other axis.
- The doc-comment on `Status` (`src/lib.rs`) now has to explain the
  exception rather than the taxonomy-is-config-owned rule applying
  uniformly.

## What would change this decision

A second project needing a `status` value roadmark doesn't ship. One
project's `blocked` was cheap to fold into the enum; a second distinct
need turns this from "one team's edge case" into "the pattern `class` and
`horizon` already went through" — at that point, revisit option B and
pay the done-ness-predicate cost once, generally, instead of adding a
fifth, sixth hardcoded variant.
