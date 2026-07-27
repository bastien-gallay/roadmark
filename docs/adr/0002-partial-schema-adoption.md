# ADR-0002 — A project may leave a schema axis out of the files entirely

- **Status:** Accepted
- **Date:** 2026-07-26
- **Deciders:** Bastien Gallay (maintainer)
- **Supersedes:** —
- **Relates to:** [ADR-0001](0001-single-source-of-truth.md); issues #25,
  #26; PRs #29, #30

## Context

[ADR-0001](0001-single-source-of-truth.md) fixed the toml/md feature files
as the single canonical store of record. An adoption question it did not
answer arrived from a project (termherd) whose priority already lived on a
GitHub Project board: **must a project hold *every* schema axis in the
files, or may it hold only some of them?**

Before this decision the answer was "every axis, in practice". `class`,
`effort` and `severity` were already omittable, but `horizon` was a
mandatory `String`, and the catalog emitted a fixed nine-column header
whatever the project declared. A board-canonical project therefore had to
either duplicate `horizon` into every feature file — two homes for one
value, reconciled by hand, which is the failure the board migration was
done to remove — or accept a catalog whose `Class/Sev`, `Effort` and
`Horizon` columns read `—` on every row. That dash is actively misleading:
it says "nobody triaged this", not "this axis is tracked elsewhere".

## Decision

**A project may leave any axis out of its feature files, and the generated
catalog reflects what the project actually holds.**

- Every taxonomy field is omittable. `horizon` joins `class`, `effort` and
  `severity` as `Option<String>`; a feature without one sorts last within
  its bucket.
- A catalog column is emitted only when **at least one feature carries a
  value for that axis**. `ID`, `Status` and `Summary` are unconditional —
  they are the table's identity, not an axis.
- `—` keeps exactly one meaning: **a gap in an axis this project does
  use**. A column that is absent means the project does not hold that axis
  at all. The two cases are no longer conflated.
- A project that wants an axis mandatory declares it: `required_when = {}`
  is unconditional, `required_when = { type = "feature" }` is conditional.

### Why this does not weaken ADR-0001

ADR-0001's guardrails apply per *field*, not per *project*:

1. *Can two systems claim to be right about the same field at the same
   time?* No. Each axis has exactly one home — the board owns `horizon`,
   the files own everything else. Partial adoption removes a duplicate
   rather than creating one.
2. *Does `validate` still make an unconditional promise?* Yes, about what
   the files hold. It never claimed to validate data that is not in them.

Mandatory `horizon` was not enforcing the SSOT invariant; it was forcing a
*second* home for a value the board already owned, which is precisely what
ADR-0001 forbids.

## Consequences

### Positive

- Board-canonical and tracker-canonical projects can adopt roadmark for the
  file layout alone, without a second source of truth.
- The catalog stops lying by omission: an empty cell and an untracked axis
  now look different.
- The rule generalises the request in issue #23 ("no `Target` column when
  `versions` is absent") instead of special-casing it.

### Negative / accepted trade-offs

- **Column presence follows the data, so it moves.** A project that
  declares an axis but has not backfilled it gets no column until the first
  feature carries a value — and that first value inserts a column into
  every row, a whole-table diff that buries the real change. It recurs in
  reverse when the last value is removed. Accepted: the alternative pins
  the Horizon column for everyone (see below).
- Optional `horizon` means a *missing* horizon can no longer be
  distinguished from a *deliberately absent* one by the schema alone.
  Mitigated by `#[serde(deny_unknown_fields)]` on `Frontmatter`: a typo
  (`horizen = "next"`) stays a parse error instead of silently reading as
  "no horizon" and dropping the feature to the end of its bucket.

  > **Superseded 2026-07-27 — the mitigation moved, the guarantee did
  > not.** `Frontmatter` no longer carries `#[serde(deny_unknown_fields)]`.
  > Project-declared fields (#22) require `#[serde(flatten)]`, which serde
  > cannot combine with it. The typo is now caught one layer out by
  > `check_declared_fields`, which reads the config and can name the
  > declaration that would make the key legal — so a mistyped `horizen`
  > still fails `generate` and is still a `validate` schema error. The
  > reasoning above stands; only the mechanism named in it is out of date.
  > `Config` and `FieldSpec` *do* carry the attribute, and should.

## Alternatives considered

| # | Option | Verdict | Why |
| --- | --- | --- | --- |
| 1 | **Column presence follows the data** (any feature carrying a value) | **Accepted** | Needs no new config key; degrades correctly during a backfill; makes #23 a special case of one general rule. |
| 2 | Column presence follows the config's `[fields.*]` declarations | Rejected | Matches the config-owned-taxonomy principle and gives stabler diffs, but `[fields.horizon]` is *mandatory* (its value order drives sort rank), so the Horizon column would stay pinned for every project — defeating the board-canonical case this ADR exists for. |
| 3 | An explicit `columns = [...]` key in `config.toml` | Rejected | More configuration for the same outcome; a project would have to remember to update it whenever it starts using an axis. |
| 4 | Keep `horizon` mandatory, document the duplicate | Rejected | Recreates by hand the drift the board migration removed. |

## Notes

Option 2 remains the better answer if `[fields.horizon]` ever stops being
mandatory — the two are coupled. If a future change lets a project declare
no horizon section at all, revisit this ADR rather than bolting a config
knob onto the data-driven rule.
