# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`import` no longer un-wraps the body it read (#71).** Reading a
  checkbox bullet joins its continuation lines, and `## Details`
  reproduces a body verbatim — so a source the adopter had wrapped at 68
  and 48 columns came out as one 104-column line in both the feature file
  and the generated `ROADMAP.md`. Every word was theirs and the line
  existed nowhere in their file, so there was nothing for them to fix:
  the same defect as #54 and #67, wearing author's clothes.
  Both halves of the split body are now re-wrapped at 80. Keeping the
  original breaks is not available — the first-sentence split does not
  align with them, and it is the split that forces the recomposition — so
  the choice was between one long line and a re-wrapped one. Nested lists
  and later paragraphs keep their own lines, which are still the
  author's.
  The commented-out `class` suggestion `import` writes was 82 columns; it
  now fits 80 once commented, so an adopter who lints their own
  `.roadmap/` has nothing to exclude.

## [0.8.1] - 2026-07-29

### Fixed

- **The 80-column rule now holds for the whole generated document
  (#67).** v0.8.0 stated that nothing `render` emits on its own exceeds
  80 columns, and shipped two lines that did. `## Details` wrote each id
  *twice* — once as the `<a id>` anchor, once as the heading text —
  around a 17-column frame, so any id past 31 characters overflowed; and
  `import` derives exactly such ids from bullet prose, so the projects
  most likely to hit it were the ones the release was written for.
  Separately, `import-leftovers.md` opened on a single 180-column
  comment.
  The anchor now sits on its own line above the heading, so the id is
  written once — the anchor line, now the binding one, fits ids up to 67
  characters where the old heading stopped at 31; the
  leftovers comment is fenced over four lines; and a slug *derived* from
  prose is bounded in characters as well as in words. An id the source
  wrote in backticks is still never truncated — that would break the
  references pointing at it.
  This changes the shape of `## Details` in every regenerated
  `ROADMAP.md`: one extra line per feature, no content difference.
  Supersedes the v0.8.0 claim below that the invariant was already
  enforced end to end — it was enforced on the banner only.

## [0.8.0] - 2026-07-28

### Added

- **`import` reads checkbox bullets, not only tables (#57).** A
  hand-written roadmap organised as checkbox bullets carrying a
  backticked id under bucket headings — arguably the most common shape a
  repo's `ROADMAP.md` actually takes — imported as nothing at all.
  Position replaces header inference: the checkbox is the status, the
  leading backticked token is the `id`, the enclosing heading is the
  `target`, and the remainder — continuation lines, nested bullets and
  further paragraphs included — is the body. The bullet form is the
  richer source, since a table cell holds one line and this holds
  paragraphs, so the first *sentence* becomes the catalog Summary and the
  rest stays in `## Details`.
  Checklists stay checklists: bullets are read only when the document
  holds no feature table, and within such a document only the ones naming
  an id — as soon as one bullet does, that is the document's convention
  and the rest are prose. Nested bullets stay in their parent's body:
  roadmark has no sub-features, and promoting them would invent ids the
  source never wrote.

### Changed

- **This repo lints its own `ROADMAP.md`.** It was in markdownlint's
  `ignores` — an exclusion #54 forced rather than the project choosing —
  and the generated document now passes. Our own feature bodies wrap at
  80 columns too, which is only usable because #55 reads the catalog
  Summary as a paragraph rather than a line. The two fixes are dogfooded
  by the repo that ships them.
- **CI regenerates `ROADMAP.md` and diffs it.** `validate` checks schema
  and *anchor* drift, not byte drift, so a renderer change that reformats
  the table — the class of bug #54 was — could land without a regenerated
  artifact and stay green. The committed document is now provably the
  output of the committed source.

### Fixed

- **An empty feature body no longer emits unlintable markdown.** An empty
  body is a `validate` *warning*, so it reaches `render` on a tree the
  tool called clean — where it produced an empty catalog cell (`|  |`,
  a table-style error) and left two blank lines under its Details heading.
  The cell now carries the `—` every other absent value gets, and the
  blank line is absorbed. Found once the generated document was linted.
- **`add`'s scaffold fits 80 columns.** Its placeholder line was 93, and
  `## Details` reproduces a body verbatim, so `add` followed by `generate`
  produced a document failing an 80-column lint — with no local signal,
  since feature files are lint-exempt. It is wrapped now, which also
  shows the wrapping the summary supports since #55.
- **The catalog's delimiter row is spaced like its header (#54).**
  `|---|---|` under a `| ID | Type |` header is an inconsistent table
  style to markdownlint (MD060), which was the last rule standing between
  the generated document and a lint run. Same complaint as the banner:
  the output has to be lintable for a project that lints its markdown.
  `import` reads either form, so a document generated by an older version
  still round-trips.
- **The generated banner wraps, so the output can pass an 80-column lint
  (#54).** `render` opened every file with one 86-character `DO NOT EDIT`
  line, so a project linting its markdown at the common 80 columns could
  not lint the artifact — and there was nothing to edit to fix it, since
  the file is regenerated. The only knob, `source_note`, *appended* to
  that line and could only make it worse; it now wraps onto its own
  lines too. Nothing `render` emits on its own exceeds 80 columns —
  verbatim author text aside, which is the project's to wrap.
- **Code spans survive into the catalog `Summary` (#59).** Backticks were
  stripped from the cell and kept in `## Details`, so the same sentence
  rendered two ways in one document and the catalog — the part most
  people read — lost the difference between a symbol and a word:
  `set_option`, `~/.config/settings.json` and `--add-dir` all arrived as
  running prose. The cell now keeps markup that carries meaning and drops
  markup that carries decoration: code spans stay, emphasis goes, and a
  link is folded to its text because the row already links to the
  feature's anchor. A span's *contents* are passed through untouched: a
  cell printing `` `__init__` `` as `` `init` `` would be worse than one
  printing `init`, because the backticks claim the mangled text is the
  symbol. A span the width truncation cut in half is dropped, opener
  included — but a backtick the author left unmatched in the source is
  prose, and stays.
- **The catalog `Summary` is the body's first *paragraph*, not its first
  line (#55).** An author wrapping that sentence — as an 80-column
  markdown house style requires — lost everything after the first line,
  silently: `validate` said nothing and `## Details` rendered the
  sentence whole, so the two halves of the generated document disagreed
  and only one was right. The paragraph's lines are now joined with
  spaces before the existing inline-markdown stripping and
  width-truncation run. A one-line summary is unaffected.

## [0.7.0] - 2026-07-27

### Added

- **`roadmark import <file>`: bootstrap `.roadmap/` from a hand-written
  roadmap.** Every candidate adopter already has a `ROADMAP.md` — that is
  the premise of the pitch — and the tool used to ask them to retype it.
  `import` reads every markdown table carrying an ID or Summary column
  and derives `id`, `status` (glyph or word), `horizon`, `area`,
  `target` (from a column, or the enclosing `##` heading when the
  document is bucketed) and the body. Headers are matched by name and a
  short alias list; `--map field=Header` overrides, repeatably.
  `--dry-run` reports and writes nothing.
  What the table can't say splits along the line the schema draws:
  `class` and `effort` are optional and are written commented out with
  their value set inline, while `type`, `area` and `target` are
  mandatory — a comment there produces a file that doesn't parse — so
  they get a `<TODO>` placeholder. The result generates immediately and
  `validate` names what is undecided rather than refusing the tree,
  which is what makes the first run useful instead of a wall. Nothing is
  overwritten: existing feature files are skipped and reported,
  `config.toml` is written only when absent, and unattributable prose
  goes to `import-leftovers.md` rather than being dropped.
  ([#24](https://github.com/bastien-gallay/roadmark/issues/24))
- **`validate` warns about `<TODO>` placeholders.** `add` and `import`
  both scaffold them, and left alone they ship into the catalog as if
  someone had decided them. A warning, not an error — scaffolding first
  and filling in second is the normal shape of adoption. The quick start
  now exits 0 with two warnings until the scaffold is filled in.

- **Project-declared fields.** A `[fields.X]` naming something roadmark
  doesn't model now declares a field of the project's own — the schema
  had no home for a tracking issue, an owner, a spec URL, so they lived
  in the free-text body where nothing could validate them, tabulate
  them, or project them anywhere. `kind` (`integer` / `string` / `url` /
  `issue-ref`) checks shape where `values` can't enumerate a set,
  `column` renders the field in the catalog, and `link` turns each value
  into a link by substituting it for `{}` — which keeps the forge out of
  the binary. `issue-ref` accepts `42` and `"#42"` alike and stays a
  plain number as far as roadmark is concerned; only a projection needs
  to know it means an issue. Declared columns land before `Summary` and
  follow ADR-0002 like every axis: no feature carries the field, no
  column. There is deliberately no `pattern` — roadmark carries no regex
  dependency and a half-regex would be worse than none.
  ([#22](https://github.com/bastien-gallay/roadmark/issues/22))
- **`rename` and `validate` now cover narrative sections.** A
  `[F-old](#f-old)` link written in a declared section is rewritten by
  `rename` and checked by `validate`, on the same terms as a feature
  body. Sections are where cross-feature prose lives, so they are the
  likeliest home for a link to a feature — and a dead one there is
  invisible to anchor drift, which compares `<a id>` tags only.
- **`required_when` accepts a list.** `{ horizon = ["now", "next"] }`
  fires when the field holds either; multiple keys are still ANDed. The
  scalar form is unchanged and means the same as a one-element list.
- **`sections`: hand-written narrative in the generated document.**
  `generate` emitted title → banner → catalog → details, with nowhere to
  put prose that belongs to no single feature — dated triage notes, "why
  this slice", horizon commentary, which items are crowned. Adopting
  roadmark meant deleting a project's reasoning and keeping only its
  inventory. Markdown files are now declared in `config.toml` with a
  slot — `before-catalog`, `after-catalog`, `after-details` — and
  injected **verbatim**: no parsing, no reformatting, only the framing
  blank lines normalised so output doesn't depend on how the file was
  saved. Several files may share a slot and emit in declaration order.
  Under `split_by_bucket` the slots keep meaning: `before-catalog` is
  before the first section, `after-catalog` after the last. `validate`
  reports a declared-but-missing file as a hard error, and its anchor
  diff now regenerates *with* the sections, so an `<a id>` inside one is
  not mistaken for drift. Paths stay inside `.roadmap/`: an absolute
  path or a `..` component is a schema error, because a document
  assembled partly from outside the source tree can't be reproduced from
  a checkout of it.
  ([#21](https://github.com/bastien-gallay/roadmark/issues/21))
- **`split_by_bucket = true`: one catalog section per bucket.** Until
  now `versions` was only a sort key, so a roadmap organised *by* its
  buckets — MoSCoW, quarters, release trains — flattened to one long
  table with a `Target` column, losing the top-level shape a reader
  navigates by. With the flag set, `render` emits one `##`-headed
  catalog per bucket in the declared order; the bucket column drops out
  inside its own section, an empty bucket emits no heading, and features
  with no `target` collect in a trailing `Unscheduled` section. The
  column drops only where the heading carries the whole value — a single
  declared target; a multi-valued `target` keeps its cell (only the first
  entry picks the section) and so does an undeclared one (no heading can
  carry it), so splitting never hides a value.
  `## Details` stays flat and stays one list: it is anchor-addressed and
  the catalog links into it. Opt-in, because it rewrites every line of
  the generated file. Two optional labels come with it, since `versions`
  is a bucket order and not necessarily a release axis:
  `bucket_label` renames the `Target` column and `unbucketed_label`
  renames the trailing section.
  ([#35](https://github.com/bastien-gallay/roadmark/issues/35))
- **`generate -o/--output <path>`, which cannot destroy the file it
  writes.** The documented recipe `roadmark generate > ROADMAP.md` has
  the *shell* truncate `ROADMAP.md` to zero bytes before roadmark runs,
  so any failure — an unparseable feature file, a missing config — left
  the committed roadmap empty and nothing written in its place. 0.6.0's
  `deny_unknown_fields` made this fire on upgrade: a tree carrying one
  stray frontmatter key generated yesterday and destroys its roadmap
  today. `--output` renders the whole document first, then writes it via
  a sibling temp file and a rename, so a failing run leaves the previous
  file byte-identical. The write keeps what the redirect gave you: a
  symlinked destination is written *through* rather than replaced, an
  existing file keeps its permission bits, and the staged data is synced
  before the rename. stdout stays the default, so
  `roadmark generate | diff ROADMAP.md -` and existing pipelines are
  unaffected. ([#41](https://github.com/bastien-gallay/roadmark/issues/41))
- **`status = "blocked"`.** `Status` gains a fourth value for work that
  is scoped and wanted but cannot start for a reason outside the
  project — distinct from `todo`, which invites someone to pick the
  work up. Ranks between `wip` and `todo` (`⛔`), so a blocked item sorts
  near the top of its bucket instead of blending into untouched work.
  Additive for `.roadmap/` trees — existing files are unaffected — but a
  source break for library consumers: `Status` is public and a `match`
  over `Wip | Todo | Done` no longer compiles. `Status` is now
  `#[non_exhaustive]` (breaking in the same way, and for the same
  reason), so downstream matches need a wildcard arm once and further
  values cost nothing. `status` remains the
  one hardcoded taxonomy field rather than config-declared like the
  others — see [ADR-0003](docs/adr/0003-status-stays-hardcoded.md) for
  why. ([#37](https://github.com/bastien-gallay/roadmark/issues/37))
- **`validate` reports warnings as well as errors.** A second, soft tier:
  warnings are printed and counted but never change the exit code. They
  name work a human still owes the file rather than a tree that would
  generate a wrong roadmap. A run with warnings and no errors prints
  `validate: no errors, N warning(s)` and exits 0.
- **`validate` reports cross-references to feature ids nothing declares.**
  A body may say `F-terminal-images` or link `[x](#f-jsonl-viewer)`; if the
  target was deleted, mistyped, or never created, the generated roadmap
  shipped a dead link and `validate` said clean. Anchor drift could not
  catch it — drift compares the regen against the committed file, and the
  regen embeds the same dead link. The **link** form is a hard error (it
  ships a broken anchor); a **bare** `F-…` mention is a warning, because
  prose legitimately names things that are not features. Matching goes
  through `anchor_id` and the same token-boundary rule `rename` uses, so
  `F-foo` never matches inside `F-foobar`.
  ([#36](https://github.com/bastien-gallay/roadmark/issues/36))
- **`validate` reports a feature with an empty body.** The body *is* the
  summary field — `render` takes the catalog `Summary` cell from its first
  non-empty line — so an empty one renders a row that links somewhere and
  says nothing. A warning, not an error: scaffolding files first and
  filling bodies second is the normal shape of a migration. The threshold
  is emptiness only; no minimum length is invented.
  ([#38](https://github.com/bastien-gallay/roadmark/issues/38))

### Fixed

- **A `versions` order that can't be honoured is now a schema error, and
  sorting no longer disagrees with grouping about a repeated entry.**
  `versions` is a bucket order — a sort rank, and since `split_by_bucket`
  also the section order — and nothing rejected a value written twice.
  The two readers then resolved it differently: sorting kept the *last*
  index (a `HashMap` insertion detail, which is no way to read a sort
  rank), grouping the *first*, so the section order and the row order
  contradicted each other and the document silently presented features in
  an order the config did not describe. Ranking is now first-wins on both
  sides, and `validate` reports the repeat against `config.toml` rather
  than picking a behaviour. The same check rejects two headings landing
  on the same text: under `split_by_bucket` every bucket is a `##` and so
  is `unbucketed_label`, in a document that already writes `## Details`
  and the `## Feature catalog` a feature-less tree falls back to — any of
  those pairs emitted the same `##` twice, which is ambiguous navigation
  and MD024 for anyone linting their generated `ROADMAP.md`.
  ([#47](https://github.com/bastien-gallay/roadmark/issues/47))
- **`validate` no longer requires `[fields.horizon]` when no feature
  carries a horizon.** ADR-0002 settled that a project may leave an axis
  out entirely, but the validator still demanded the section
  unconditionally — so the board-canonical project that change unblocked
  could not pass the gate, and the workaround was to declare five values
  nothing used and nothing rendered. The rule is now "a `[fields.X]`
  declaration is required iff some feature actually carries `X`", using
  the same predicate that decides whether `render` emits the column, so
  the validator and the renderer cannot disagree about which axes a
  project holds. Scoped to the axes a feature may actually omit
  (`class`, `effort`, `horizon`, `severity`) — `type` and `area` are
  structurally mandatory, so requiring a declared value set for them
  would impose a taxonomy on every project rather than follow the data.
  ([#34](https://github.com/bastien-gallay/roadmark/issues/34))
- **`validate` no longer silent-passes an explicitly wrong `--root`.**
  Skipping when `.roadmap/` is absent is deliberate and stays — the same
  CI recipe must run on checkouts without the source tree. But it also
  swallowed a `--root` the user typed, so a renamed or mistyped path in a
  workflow file switched the roadmap guarantee off and kept the job
  green. An explicitly passed `--root` with no `features/` under it is now
  a hard error naming the resolved path; the defaulted root is unchanged
  and covered by a regression test. `generate`'s diagnostic now names the
  resolved root too, so both subcommands tell the same story about the
  same mistake.
  ([#31](https://github.com/bastien-gallay/roadmark/issues/31))

### Changed

- The README quick start, the `rename` hint, and the generated banner's
  `source_note` now show the `-o` form rather than the redirection.
- **`render` takes the loaded sections (breaking: library API).**
  `render(&features, &config)` becomes
  `render(&features, &config, &sections)`; pass `&[]` for none, or
  `load_sections(root, &config)?` to include them. A separate
  two-argument form was rejected deliberately: it would silently drop
  the narrative of any project whose config declares it, which is the
  exact failure the feature exists to prevent.
- **Unknown frontmatter keys are rejected one layer out (breaking:
  library API).** `Frontmatter` can no longer carry
  `deny_unknown_fields`: project-declared fields need `serde(flatten)`,
  and serde cannot combine the two. The guarantee is unchanged in
  substance — an undeclared key still fails `generate` and is still a
  `validate` schema error — and the message now names the declaration
  that would make the key legal instead of just refusing it. The cost is
  in the API: `Frontmatter` gains a public `extra` map, `load_features`
  takes the config (`load_features(root, &config)`) because the check
  needs it, and `Frontmatter`/`Feature` lose their `Eq` impl since
  `toml::Value` has none. A `[fields.X]` outside the built-in set is no
  longer a config error — it is a declaration — so that check is gone;
  what replaced it validates the declaration's own coherence (`values`
  and `kind` together, neither of them, `link` without `column`).
- **Unknown keys in `config.toml` are rejected (breaking).**
  `Config` and `[fields.*]` now carry `deny_unknown_fields`, as
  `Frontmatter` has since 0.6.0. Every config key is optional, so a typo
  had no shape to fail on and read as "the user didn't want that". TOML
  sharpens it: a top-level key written *below* a `[fields.x]` table
  belongs to that table, so a misplaced `sections = [...]` silently
  became `fields.x.sections` and the narrative it declared never
  appeared — found while writing the tests for this release. A config
  carrying a key roadmark doesn't model now fails at parse time instead.
- **`Config` gained four fields (breaking: library API).**
  `split_by_bucket`, `bucket_label`, `unbucketed_label` and `sections`.
  `.roadmap/` trees are unaffected — all four default to off or empty —
  but a struct literal `Config { .. }` in downstream code no longer
  compiles. `Config` now implements `Default`, so `..Config::default()`
  covers these additions and the next one.

## [0.6.0] - 2026-07-26

### Changed

- **A project may leave a schema axis out of its feature files
  entirely (breaking: library API).** `horizon` joins
  `class`/`effort`/`severity` as an optional field — a feature without
  one is valid and sorts last within its bucket. `Frontmatter::horizon`
  is now `Option<String>` rather than `String`, so library consumers
  reading or constructing that field need updating; `.roadmap/` trees
  themselves are unaffected. `validate` still enforces membership when
  the field is present, and a project that wants it mandatory declares
  `required_when = {}` (unconditional) or a condition such as
  `required_when = { type = "feature" }`. Supersedes the 0.2.0 note that
  every feature carries a horizon. See
  [ADR-0002](docs/adr/0002-partial-schema-adoption.md).
- **Catalog columns follow the data (breaking: output shape).** A
  column is emitted only when at least one feature carries a value for
  that axis; `ID`, `Status` and `Summary` stay unconditional. `—` now
  means "a gap in an axis this project uses", never "this project does
  not track this axis". Supersedes the 0.3.0 note that the catalog
  gains Type/Class/Sev/Effort/Horizon columns unconditionally. **Any
  project not using all six axes will see its `ROADMAP.md` change shape
  on the next regen** — expect one whole-table diff, then stability.
- **Unknown frontmatter keys are rejected (breaking).**
  `#[serde(deny_unknown_fields)]` on the frontmatter: a stray or
  mistyped key is now a parse error. Without it, a typo (`horizen =
  "next"`) would silently read as "no horizon" and drop the feature to
  the end of its bucket, which optional `horizon` made possible.

### Fixed

- `required_when = {}` (the unconditional form) reported
  `` `horizon` is required when `` with nothing after "when"; it now
  reports `` `horizon` is required ``.
- The `Config::versions` doc-comment promised "sorting and section
  emission"; `render` has never emitted per-bucket sections, so the
  comment now says sorting only. No behaviour change.

## [0.5.1] - 2026-07-13

### Added

- **Automated crates.io publishing from CI via Trusted Publishing
  (OIDC).** Pushing a `v<semver>` tag now publishes the crate with no
  long-lived token stored anywhere: the dist-generated release workflow
  runs a custom publish job that mints an ephemeral crates.io token per
  run (`rust-lang/crates-io-auth-action`) and runs `cargo publish`. This
  replaces the manual `cargo login` / `cargo publish` step. Requires a
  one-time Trusted Publisher config on crates.io.

## [0.5.0] - 2026-07-12

### Added

- **First release published to crates.io** — install with
  `cargo install roadmark`. The published crate is trimmed via an
  `include` allowlist to sources, README, changelog, and the license
  pair.

### Changed

- **Renamed the project to `roadmark`.** The crate, the library, the
  binary, and the GitHub repository (`bastien-gallay/roadmark`) are all now
  `roadmark`; the command you invoke changes from `roadmap …` to
  `roadmark …`. The `.roadmap/` source directory and the generated
  `ROADMAP.md` keep their names. The `roadmark`-named release artifacts
  ship with the first release cut after the rename.

## [0.4.0] - 2026-07-12

### Added

- `roadmap rename <from> <to>` — rename a feature: move its file, update
  the frontmatter `id`, and rewrite cross-references (`[F-old](#f-old)`
  links, bare id mentions, and `f-old.md` path references) across every
  feature body via whole-token replacement. Refuses to overwrite an
  existing file, to collide with another feature's anchor, or to run
  while the old id is duplicated; legacy `f<digits>` targets require
  `--allow-legacy-numeric`.

### Changed

- The catalog Summary column is now a scannable plain-text lead: inline
  markdown is stripped (code spans, `*`/`_` emphasis, `[text](url)` links
  folded to text), whitespace collapsed, and the text truncated to 120
  chars on a word boundary. The full raw body still renders under
  `## Details`.

## [0.3.0] - 2026-07-12

### Added

- The generated `ROADMAP.md` now surfaces the schema-v2 fields: the
  catalog table gains Type, Class/Sev, Effort and Horizon columns, and
  a new Details section renders each feature's full markdown body plus
  a "Shipped in …" line from the `shipped` metadata.

## [0.2.0] - 2026-07-12

Schema v2 — **breaking change** to the feature-file frontmatter.

### Changed

- Frontmatter schema v2: two orthogonal axes replace the old flat
  `priority` — `class` (kind of leverage, feature-only) and `effort`
  (S/M/L). `topic` becomes the multi-valued `area`; `priority` becomes
  `horizon`; `type` (`feature | fix | chore`) and fix-only `severity`
  are new.
- Allowed values for `type`/`class`/`effort`/`area`/`horizon`/`severity`
  are no longer hardcoded: each project declares them in `config.toml`
  `[fields.*]` (closed value set, `multi` shape, `required_when`
  conditions). `validate` enforces the declarations.
- Sort key is now target bucket → status → horizon (declared value
  order) → `shipped_order` → id.

### Fixed

- `required_when` evaluates every condition key (AND semantics), not
  only `type`.
- A single-valued field given a TOML list is now a schema error (the
  `multi` flag is enforced).
- An unknown `[fields.*]` name in `config.toml` is rejected instead of
  silently disabling that field's validation.
- `[fields.horizon]` is required: every feature carries a horizon and
  it drives the sort order.

## [0.1.0] - 2026-07-11

Initial release.

### Added

- `roadmap generate` — render `ROADMAP.md` to stdout from a `.roadmap/`
  directory of TOML-frontmatter feature files (deterministic output).
- `roadmap validate` — schema errors, duplicate ids, anchor collisions,
  anchor drift against the committed `ROADMAP.md` (`--accept-drift`).
- `roadmap add` — scaffold a feature file (`f-<kebab-name>`; legacy
  `f<digits>` behind `--allow-legacy-numeric`).
- CRLF-authored feature files parse correctly.
- Prebuilt binaries for 5 targets plus shell/powershell installers
  (cargo-dist).

[Unreleased]: https://github.com/bastien-gallay/roadmark/compare/v0.8.1...HEAD
[0.8.1]: https://github.com/bastien-gallay/roadmark/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/bastien-gallay/roadmark/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/bastien-gallay/roadmark/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/bastien-gallay/roadmark/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/bastien-gallay/roadmark/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/bastien-gallay/roadmark/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/bastien-gallay/roadmark/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/bastien-gallay/roadmark/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/bastien-gallay/roadmark/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bastien-gallay/roadmark/releases/tag/v0.1.0
