# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.6.0]: https://github.com/bastien-gallay/roadmark/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/bastien-gallay/roadmark/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/bastien-gallay/roadmark/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/bastien-gallay/roadmark/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/bastien-gallay/roadmark/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/bastien-gallay/roadmark/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/bastien-gallay/roadmark/releases/tag/v0.1.0
