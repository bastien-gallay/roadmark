# roadmark

[![CI](https://github.com/bastien-gallay/roadmark/actions/workflows/ci.yml/badge.svg)](https://github.com/bastien-gallay/roadmark/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/roadmark.svg)](https://crates.io/crates/roadmark)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

**Your roadmap as code — compiled, versioned, and validated in CI, so it
never rots.**

> **Naming:** the crate, the binary, and the GitHub repository
> (`bastien-gallay/roadmark`) are all `roadmark`.

---

> **We believe** the hand-maintained `ROADMAP.md` is rotting debt: every
> sprint it drifts from the code, lies to contributors, and ends up
> abandoned in a corner of the repo.
> **We believe** planning deserves the same rigor as code — compiled,
> versioned, automatically validated — not held together by human goodwill.
> **That's why** roadmark compiles your roadmap from atomic feature files
> and **breaks your CI** the moment it becomes inconsistent. Discipline
> becomes mechanical, not moral.

Built to scratch my own itch — this repo dogfoods roadmark on its own
`.roadmap/`.

---

## What it is

roadmark is a **roadmap-as-code** tool for teams that live in their Git
repo. Instead of coordinating edits on one big roadmap file, each feature is
its own markdown file with TOML frontmatter. One command compiles them into
a `ROADMAP.md`; another **guarantees** the roadmap can't become incoherent —
enforced in CI, not by discipline.

It is **not** a task tracker (leave day-to-day tasks to tools like
Backlog.md), and **not** a hosted roadmap app (OpenProject / Productboard
style). It sits in the gap none of them fill: docs-as-code, at the roadmap
level, with a validation guarantee.

## Install

From crates.io:

```sh
cargo install roadmark
```

Or grab a prebuilt binary (macOS, Linux, Windows — see the
[latest release](https://github.com/bastien-gallay/roadmark/releases/latest)):

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/bastien-gallay/roadmark/releases/latest/download/roadmark-installer.sh | sh
```

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/bastien-gallay/roadmark/releases/latest/download/roadmark-installer.ps1 | iex"
```

Or build from the Git repo or a local checkout:

```sh
cargo install --git https://github.com/bastien-gallay/roadmark
cargo install --path .
```

All of these install a binary named `roadmark`.

## Quick start

There is no `init` yet ([F-init](ROADMAP.md#f-init) is planned) and `add`
does not write a config, so start by creating `.roadmap/config.toml`
yourself — this is the smallest one that works:

```toml
# .roadmap/config.toml
versions = ["v0.1", "Later"]     # sort buckets, earliest first

[fields.horizon]                 # `add` scaffolds a horizon, so declare it:
values = ["now", "next", "later", "shipped"]   # order = rank
```

Then:

```sh
roadmark add f-dark-mode          # scaffold a new feature file under .roadmap/features/
roadmark generate -o ROADMAP.md   # compile features into ROADMAP.md
roadmark validate                 # fail if the roadmap is inconsistent — run this in CI
```

`roadmark --root path/to/.roadmap generate` points at a non-default location.

> **Use `-o`, not `>`.** `generate` still writes to stdout by default, so
> `roadmark generate | diff ROADMAP.md -` and other pipelines keep working.
> But `roadmark generate > ROADMAP.md` has the *shell* empty the file before
> roadmark runs, so a failed generate leaves you with nothing rather than the
> previous roadmap. `-o/--output` writes via a temp file and a rename, so a
> failing run leaves the existing file untouched.

The taxonomy above is deliberately minimal. Every other axis — `type`,
`class`, `effort`, `area`, `severity` — is optional, and any axis you leave
out of your feature files gets no column in the generated catalog. You only
owe a `[fields.X]` section for an axis your features actually carry; see
[Author](#1-author-the-body-rich-git-native-roadmap-management) for the
full config.

---

## How it works — three layers

### 1. Author (the body): rich, Git-native roadmap management

One feature = one file. Two people never edit the same line, so the roadmap
has **zero merge conflicts**. Each feature carries structured frontmatter
plus a free markdown body whose first non-empty line becomes the catalog
summary. The taxonomy is **yours**: statuses, effort levels, horizons, and
areas are declared in `config.toml`, with no process religion baked in.

The tool reads a `.roadmap/` directory (override with `--root`):

```text
.roadmap/
├── config.toml
└── features/
    ├── f-dark-mode.md
    ├── f-another-thing.md
    └── ...
```

Each `features/*.md` is TOML frontmatter fenced by `+++`, followed by the
markdown body:

```markdown
+++
id = "F-dark-mode"
type = "feature"        # feature | fix | chore
class = "enabler"       # feature-only leverage (see [fields.class])
effort = "M"            # S | M | L
area = ["core", "cli"]  # multi-valued taxonomy
horizon = "next"        # optional; absent rows sort last in bucket
status = "todo"         # wip | blocked | todo | done
target = ["v0.2"]       # first entry drives the sort bucket
+++

One-paragraph summary — the first non-empty line lands in the Summary column.
```

Frontmatter keys roadmark neither models nor sees declared in your config
are rejected: with `horizon` optional, a typo (`horizen = "next"`) would
otherwise silently read as "no horizon" and drop the feature to the end
of its bucket. Keys you *do* declare are yours — see
[your own fields](#your-own-fields).

A fix carries a `severity` instead of a `class`:

```toml
id = "F-broken-anchor"
type = "fix"
severity = "major"      # fix-only (see [fields.severity])
area = ["core"]
horizon = "now"
status = "wip"
target = ["v0.2"]
```

When a feature flips to `status = "done"`, record its shipping metadata so
historical order survives every regen:

```toml
shipped = { version = "v0.1", date = "2026-07-11", pr = 42 }
shipped_order = 3       # stable position within the shipped tier
```

The allowed values for every field are **config-owned, not hardcoded** —
the generator stays taxonomy-neutral so roadmark is reusable across
projects. `status` (`wip | blocked | todo | done`) is the one exception:
it drives sort order and the shipped tier, so it stays a fixed enum — see
[ADR-0003](docs/adr/0003-status-stays-hardcoded.md):

```toml
# .roadmap/config.toml
versions = ["v0.1", "v0.2", "v0.3", "Later"]   # sort buckets, earliest first
title = "My Project — Roadmap"                  # H1 of the generated doc
split_by_bucket = false                         # one catalog per bucket (see Generate)

[fields.type]
values = ["feature", "fix", "chore"]

[fields.class]
values = ["differentiator", "enabler", "table-stakes", "polish", "bet"]
required_when = { type = "feature" }            # class only on features

[fields.effort]
values = ["S", "M", "L"]

[fields.area]
values = ["core", "docs", "cli"]
multi = true

[fields.horizon]
values = ["now", "next", "later", "parked", "shipped"]   # order = sort rank
# `horizon` is optional per feature (e.g. priority lives on a board).
# To make it mandatory again use `required_when = {}` (unconditional) or
# a condition such as `required_when = { type = "feature" }` — the latter
# leaves it optional for the other types.

[fields.severity]
values = ["critical", "major", "minor"]
required_when = { type = "fix" }
```

`required_when` takes a single value or a list — `{ horizon = ["now",
"next"] }` fires when `horizon` is either. Multiple keys are ANDed.

#### Your own fields

A `[fields.X]` naming something roadmark doesn't model declares a
**project field**. Use `kind` instead of `values` when the values can't
be enumerated — a tracking issue number has no value set:

```toml
[fields.tracked]
kind = "issue-ref"                              # integer | string | url | issue-ref
required_when = { horizon = ["now", "next"] }   # a live feature must be tracked
column = "Tracked"                              # render it in the catalog
link = "https://github.com/owner/repo/issues/{}"   # …as a link
```

```toml
# frontmatter
tracked = 42     # or "#42" — both link to issue 42
```

`kind` is deliberately shallow, and there is no `pattern`: roadmark
carries no regex dependency, and a half-regex would be worse than none.
Anything finer belongs in your own CI. `issue-ref` stays a plain number
as far as roadmark is concerned — only a projection needs to know it
means an issue.

Declared columns land just before `Summary` and follow the same rule as
every axis: no feature carries the field, no column. Values are
percent-encoded into the `link` template, so a free-text value like
`Jane Doe (ops)` still produces a working link.

Two names you can't declare. `id`, `status`, `target`, `shipped` and
`shipped_order` are core schema, not taxonomy axes — declaring one
constrains nothing (`status` in particular is deliberately hardcoded,
see [ADR-0003](docs/adr/0003-status-stays-hardcoded.md)). And `column`
on a built-in axis is refused: it already has one, so the declaration
would print the same value twice. Both are schema errors.

**Unknown keys are still rejected.** A frontmatter key that no
`[fields.*]` declares fails `generate` and is a `validate` schema error,
naming the declaration that would make it legal. With every axis
optional, a typo would otherwise read as an absent field.

### 2. Generate: the roadmap is a compiled artifact

`roadmark generate` compiles every feature file into a single, formatted
`ROADMAP.md` — on stdout by default, or to a path with `-o/--output`, which
writes through a temp file and a rename so a failing run cannot destroy the
roadmap it was regenerating. The output has two parts — a **feature catalog**
(one table row per feature, ID linking to its detail section) and
**details** (each feature's full body, verbatim). It is **deterministic**:
the catalog is sorted by a total key (target bucket → status → horizon →
`shipped_order` → id), so regeneration is byte-stable and diffs stay clean.
Catalog columns for axes no feature uses (e.g. `Target` or `Effort` in a
project that never sets them) are omitted; a partially used axis keeps
its column, with `—` for features that carry no value.

#### One catalog per bucket

By default the catalog is a single table and `versions` only sorts it.
A roadmap whose *shape* is its buckets — MoSCoW, quarters, release
trains — can get one `##` section per bucket instead:

```toml
versions = ["Must", "Should", "Could", "Backlog"]
split_by_bucket = true            # default false
bucket_label = "Priority"         # optional: renames the `Target` column
unbucketed_label = "Unsorted"     # optional: heading for the tail section
```

```markdown
## Must

| ID | Type | Area | Status | Summary |
|---|---|---|---|---|
…

## Should
…

## Details
```

Sections follow the declared `versions` order — the same order the sort
uses, so rows keep their global ordering. The bucket column drops out
inside its own section (the heading already carries it), exactly as an
unused axis drops its column — but only when the heading carries the
*whole* value: a single, declared target. A multi-valued
`target = ["v0.2", "v0.3"]` keeps its cell, because only the first entry
picks the section, and so does a target `versions` doesn't declare,
because no heading can carry it at all. Splitting never hides a value.

A declared-but-empty bucket emits no heading. Features with no `target`
— or an undeclared one — collect in a trailing **Unscheduled** section. A
project with no features yet keeps the flat `Feature catalog` heading:
there is nothing to split.

`## Details` stays flat and stays one list: it is anchor-addressed and
the catalog links into it, so splitting it would double the anchor
surface for no navigational gain.

It is opt-in because it rewrites every line of the generated file. For a
project with few features, or no meaningful bucket axis, the flat table
is the right output.

`versions` names document positions, so two ways of writing it are
schema errors `validate` reports against `config.toml`:

- **a repeated entry** — `["v1", "v2", "v1"]`. A bucket can only hold
  one position; the first occurrence is what sorting and grouping both
  honour, but the config is describing an order the document doesn't
  have. Reported in flat mode too, where it still shifts rows into a
  bucket position nobody wrote down
- **a bucket named like a heading `generate` writes itself** —
  `Details`, `Feature catalog`, or whatever `unbucketed_label` resolves
  to (`Unscheduled` by default). Under `split_by_bucket` that emits the
  same `##` twice: ambiguous navigation, and MD024 if you lint the
  generated file

#### Hand-written narrative

Some prose belongs to no single feature: dated triage notes, "why this
slice", horizon commentary, which items are crowned and in what order.
It is *about* the relationships between features, so it fits in no
feature body — and moving it to `docs/` means neither file is the
roadmap any more.

Declare markdown files and where they land:

```toml
sections = [
  { file = "preamble.md", slot = "before-catalog" },
  { file = "notes.md",    slot = "after-catalog" },
  { file = "closing.md",  slot = "after-details" },
]
```

```text
.roadmap/
├── config.toml
├── preamble.md
├── notes.md
└── features/
```

Three slots, named for the document's structural landmarks:
`before-catalog` (after the title and banner), `after-catalog` (before
`## Details`), and `after-details` (the end). Under `split_by_bucket` the
catalog is several sections, so `before-catalog` means before the
*first* and `after-catalog` after the *last* — the boundaries hold
whatever shape the catalog takes. Several files may share a slot; they
emit in declaration order.

Two properties make this safe to rely on:

- **Verbatim.** roadmark neither parses nor reformats the content —
  fenced blocks, tables and HTML survive as written. Only the *framing*
  is normalised: leading and trailing blank lines are dropped, so the
  document's spacing doesn't depend on how your editor saved the file.
- **Counted by `validate`.** A declared file that isn't there is a hard
  error, not a silent hole. `generate` would fail outright, so a passing
  `validate` would be promising a document the next command refuses to
  produce.
- **Held to the same cross-reference rules as a feature body.** A
  `[F-foo](#f-foo)` link in a section is checked by `validate` and
  rewritten by `rename`. Sections are where cross-feature prose lives, so
  they're the *likeliest* home for a link to a feature — and a dead one
  there is invisible to anchor drift, which only compares `<a id>` tags.

Paths are relative to the `.roadmap/` root and must stay inside it: an
absolute path or a `..` component is a schema error. `.roadmap/` is the
source of truth, and a document assembled partly from outside it can't
be reproduced from a checkout of it.

### 3. Validate — the guarantee

This is the point. `roadmark validate` is read-only. It reports two tiers.

**Hard errors** — the tree would generate a roadmap that is *wrong*. These
fail the run:

- **schema errors** — malformed frontmatter, unknown field values, a
  single-valued field given a list, a missing `required_when` field, or a
  `[fields.X]` declaration missing for an axis some feature actually
  carries, or a `versions` order that repeats an entry or collides with a
  structural heading
- **duplicate ids / anchor collisions** — two features that would produce
  the same `<a id="…">` anchor (checked case-insensitively)
- **dangling links** — a body links `](#f-something)` at a feature id
  nothing declares, so the generated roadmap ships a dead anchor. Anchor
  drift cannot catch this: the regen contains the same dead link, so the
  two agree
- **a missing `--root`** — see below
- **anchor drift** — anchors the committed `ROADMAP.md` is missing or has
  stale, i.e. you forgot to regenerate (pass `--accept-drift` to downgrade
  to a warning)

**Warnings** — worth naming, never fatal. They are printed, and the exit
code stays 0:

- **an empty body** — the catalog `Summary` cell is the body's first
  non-empty line, so a feature with no body renders a row that links
  somewhere and says nothing. A warning rather than an error because
  scaffolding files first and writing bodies second is the normal shape of
  a migration
- **a bare reference to an unknown id** — prose saying `F-something` that
  no feature declares. Softer than the link form, because prose
  legitimately names things that are not features

Wire it into CI and your roadmap **cannot** silently drift or lie:

```yaml
# .github/workflows/roadmap.yml  (sketch)
- run: roadmark validate    # the PR fails if the roadmap is inconsistent
```

`validate` silently passes when `.roadmap/` is absent, so the same recipe
runs on checkouts without the source tree. That escape hatch applies to the
**default** root only: if you pass `--root` explicitly and the tree is not
there, that is a typo, and `validate` fails naming the path it resolved
rather than reporting a clean run that checked nothing.

## Other commands

```sh
roadmark rename f-old f-new      # move a feature file, rewriting every cross-link
```

`rename` moves `features/<from>.md` to `features/<to>.md`, updates its `id`,
and rewrites cross-references (`[F-old](#f-old)` links, bare id mentions,
`f-old.md` path references) in every feature body. Matching is whole-token,
so ids that merely share a prefix (`F-old-widget`) are untouched. It refuses
to overwrite an existing file, to collide with another feature's anchor, or
to run while the old id is duplicated. Regenerate `ROADMAP.md` afterwards.

New features use the `f-<kebab-name>` slug shape. The legacy `f<digits>`
form is rejected by `add` (and as a `rename` target) unless
`--allow-legacy-numeric` is passed (migration only).

---

## Reach — headless roadmap

Your roadmap's source of truth stays in Git; it **projects** to wherever
your team already works. Like a headless CMS, the canonical content lives in
one clean, versioned place and is rendered where it's needed.

| Projection | Direction | Status |
| --- | --- | --- |
| `ROADMAP.md` | files → doc | ✅ available |
| GitHub Projects | files → board | 🔭 planned (demand-driven) |
| Jira | files ↔ tool | 🔭 planned (demand-driven) |

**Design invariant:** the toml/md files are the single source of truth;
every backend is a projection reached through a **one-way** adapter, and no
external tool is ever co-authoritative. See
[`docs/adr/0001-single-source-of-truth.md`](docs/adr/0001-single-source-of-truth.md).
This is exactly what makes `validate`'s promise unconditional — the source
is guaranteed clean *before* it propagates anywhere.

---

## Why not just…

- **…a hand-edited `ROADMAP.md`?** It rots. It drifts from the code, causes
  merge conflicts, and no one trusts it after a month. roadmark makes
  coherence mechanical.
- **…a SaaS roadmap tool?** It lives outside the repo, invisible in code
  review, and disconnected from the code it describes. roadmark keeps the
  roadmap in the PR — and can still project *to* your SaaS if your team
  needs it.
- **…a markdown task manager (Backlog.md, etc.)?** Those track tasks;
  roadmark plans the roadmap above them. They're complementary, not
  competing.

## Status

Early and actively dogfooded. The core (`add` / `generate` / `validate` /
`rename`) is shipped and complete; external projections (GitHub Projects,
Jira) are planned and demand-driven. Issues and feedback welcome.

**Pre-1.0, so breaking changes happen** — in the frontmatter schema, in
the shape of the generated catalog, and in the library API. 0.6.0 carried
all three. Every one is documented in the
[changelog](CHANGELOG.md) under the release that introduced it; read it
before upgrading, and pin a version if a regenerated `ROADMAP.md`
appearing in your diff would be disruptive.

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
