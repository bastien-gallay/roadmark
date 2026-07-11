# roadmap-cli

Generate a `ROADMAP.md` from a `.roadmap/` directory of TOML-frontmatter
feature files. The roadmap document becomes a **generated artifact**; the
source of truth is one small markdown file per feature, so roadmap edits
skip the usual "edit a big shared file" merge ceremony.

The binary is called `roadmap`.

## Install

```sh
cargo install --git https://github.com/bastien-gallay/roadmap-cli
```

Or from a local checkout:

```sh
cargo install --path .
```

## Source layout

The tool reads a `.roadmap/` directory (override with `--root`):

```text
.roadmap/
├── config.toml
└── features/
    ├── f-my-feature.md
    ├── f-another-thing.md
    └── ...
```

### `config.toml`

```toml
# Bucket order for sorting and section emission. Earliest cycle first.
versions = ["v0.1", "v0.2", "v0.3", "Later", "Speculative"]

# H1 heading of the generated ROADMAP.md. Optional, defaults to "Roadmap".
title = "My Project — Roadmap"

# Optional note appended to the generated "DO NOT EDIT" banner —
# e.g. a pointer to an ADR or design doc. Optional.
source_note = "See docs/adr/roadmap-pipeline.md for the design."
```

### A feature file

Each `features/*.md` is TOML frontmatter fenced by `+++`, followed by a
markdown body whose first non-empty line becomes the catalog summary:

```markdown
+++
id = "F-my-feature"
topic = "Architecture"
status = "todo"    # wip | todo | done
priority = "next"  # next | later | speculative | shipped
target = ["v0.2"]  # first entry drives sort bucket
+++

One-paragraph summary — the first non-empty line lands in the Summary column.
```

## Commands

```sh
roadmap generate > ROADMAP.md   # render to stdout
roadmap validate                # schema, slug uniqueness, anchor drift
roadmap add f-my-feature        # scaffold a new feature file
```

`roadmap --root path/to/.roadmap generate` points at a non-default location.

### `validate`

Read-only. Reports:

- **schema errors** — malformed frontmatter (does not abort on the first one)
- **duplicate ids** / **anchor collisions** — two features that would
  produce the same `<a id="…">` anchor
- **anchor drift** — anchors in the committed `ROADMAP.md` that a fresh
  regen would add or drop (catches "forgot to regenerate"). Pass
  `--accept-drift` to downgrade drift to a warning.

Exit code is non-zero on hard errors, or on drift unless `--accept-drift`.

## Slug convention

New features use `f-<kebab-name>`. The legacy `f<digits>` form is rejected
by `add` unless `--allow-legacy-numeric` is passed (migration only).

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
