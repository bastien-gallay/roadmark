# ADR-0001 — The toml/md feature files are the single source of truth

- **Status:** Accepted
- **Date:** 2026-07-12
- **Deciders:** Bastien Gallay (maintainer)
- **Supersedes:** —

## Context

roadmark treats a roadmap as a *compiled artifact*: each feature lives in
its own markdown file with TOML frontmatter under `.roadmap/features/`,
governed by `.roadmap/config.toml`. `roadmark generate` compiles these into
a `ROADMAP.md`; `roadmark validate` guarantees the roadmap cannot become
incoherent (schema violations, duplicate ids, anchor collisions,
regenerate-drift).

As adoption grows toward teams (not just solo maintainers), a recurring
need appears: people who do not edit `.md` files — e.g. a Product Owner
working in a web tool — and teams already living in an external system
(GitHub Projects, or Jira for a possible future client). This raises a
foundational question: **where does the single source of truth (SSOT)
live, and in which direction may data flow?**

Five options were considered (see *Alternatives*). The decision matters
because the product's core differentiator — `validate` guaranteeing that
"the roadmap cannot lie" — only holds if there is exactly one place of
authority. Any design that makes the SSOT configurable or lets two systems
co-own the same data turns that guarantee into a conditional one and opens
a conflict-resolution problem that is disproportionate for the project's
scope.

A key reframing unblocked the decision: **the *store of record* and the
*authoring surface / sync direction* are two independent layers.** The
store of record is decided once; sync direction is decided per adapter, as
concrete needs are confirmed.

## Decision

**The individual toml/md feature files are the single, canonical store of
record. Permanently.**

- `validate` always runs against these files and keeps an **unconditional**
  guarantee.
- All external backends (`ROADMAP.md`, GitHub Projects, Jira, …) are
  **projections** reached through adapters.
- Every adapter is **explicitly one-way**:
  - *outbound* = projection (files → external tool, for visibility),
  - *inbound* = write-back (external tool → files, for non-dev authoring),
    the files remaining canonical and re-validated after any write-back.
- **No external backend is ever co-authoritative.** There is no
  bidirectional, field-level merge between two systems of record.

### Shipping order

1. Shipped: outbound projection to `ROADMAP.md` (files → doc).
2. Planned, on confirmed demand: further outbound projections
   (files → GitHub Projects; files → Jira).
3. Planned, on confirmed demand: inbound write-back (e.g. a PO authoring in
   a web tool → files).
4. Out of scope: configurable SSOT, and any-direction / bidirectional sync.

## Decision guardrail (apply to every future sync feature)

1. **"Can two systems claim to be right about the same field at the same
   time?"**
   No (one-way, one store of record) → safe, ship it.
   Yes (co-authority) → this signs us up to build conflict resolution. Do
   not, unless a paying customer funds exactly that.
2. **"After this feature ships, does `validate` still make an
   *unconditional* promise?"**
   If the promise becomes "valid, provided you configured X and sync did
   not clobber Y", the moat is damaged. Reject.

## Consequences

### Positive

- The `validate` guarantee stays unconditional — the core differentiator is
  protected.
- No conflict-resolution engine to build or maintain (a tar pit for a solo
  maintainer).
- Git-native invariants preserved: diffs, reviews, zero merge conflicts on
  the roadmap.
- Matches the maintainer's own daily use (one canonical way to manage
  roadmaps).
- Still serves "team" needs (web-tool PO, GitHub Projects, Jira) via
  one-way adapters — ~90% of the flexibility of a configurable/bidirectional
  design, without its cost.

### Negative / accepted trade-offs

- Teams whose PO edits *only* in a web tool are not served until an inbound
  write-back adapter exists (option deferred, not refused).
- Some projections are lossy (an external tool may not represent every
  field); this is documented per adapter rather than solved by a universal
  mapping.

### Neutral

- Reframes sync as an *adapter* concern, not an *architecture* concern: new
  backends are additive and do not touch the foundational invariant.

## Alternatives considered

| # | Option | Verdict | Why |
| --- | --- | --- | --- |
| 1 | SSOT in the repo, coarse-grained | Rejected | Dominated by #2 — coarser, loses per-file granularity that powers both `validate` and zero-merge-conflict. |
| 2 | **SSOT = individual toml/md, outbound projection** | **Accepted (default)** | Preserves the guarantee, expresses a clear opinion, justified by confirmed need, matches dogfooding. |
| 3 | SSOT = toml/md + one-way inbound write-back | Accepted as additive | Not a rival to #2 — it is #2 plus an inbound adapter. Built only on confirmed PO-authoring demand. |
| 4 | SSOT configurable at install | Rejected | Refuses to have an opinion on the most fundamental question; makes `validate` conditional; would never be used by the maintainer; expensive to reverse. |
| 5 | Sync verbs in any direction | Partially rejected | Keep the one-way verb subset; refuse the bidirectional/co-authoritative part — it is a conflict-resolution product and a scope/identity drift toward an "integrations hub" red ocean. |

## Notes

Adopting #4 or #5 as the product's *identity* would turn roadmark into a
"Zapier for roadmaps" — a different moat (breadth of integrations), fierce
competition, and no advantage. The confirmed differentiator is integrity +
validation on a single Git-native source of truth. This ADR exists to keep
that invariant stable as backends multiply.
