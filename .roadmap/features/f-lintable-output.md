+++
id = "F-lintable-output"
type = "fix"
severity = "major"
effort = "M"
area = ["core"]
horizon = "shipped"
status = "done"
target = ["v0.8"]
shipped = { version = "v0.8.0", date = "2026-07-28", pr = 64 }
shipped_order = 18
+++

The generated document cohabits with an 80-column markdown lint, and its two
halves render the same sentence the same way.

Four defects, one complaint, all found migrating a real project onto roadmark.
The catalog Summary was the body's first *line*, so an author wrapping that
sentence — as an 80-column house style requires — lost everything after it,
silently, while `## Details` rendered it whole. The banner was one 86-column
line with nothing to edit, since the file is regenerated. The delimiter row
`|---|` under a `| ID |` header was an inconsistent table style. And code
spans were stripped from the cell but kept in Details, so the catalog lost the
difference between a symbol and a word.

The rule that came out of it: nothing the renderer emits on its own may exceed
80 columns, and a cell keeps the markup that carries meaning while dropping
the markup that carries decoration. Both are enforced rather than remembered —
there is a per-line assertion on the banner, and CI regenerates `ROADMAP.md`
and diffs it, because `validate` sees schema and anchor drift but not byte
drift.

This repo now lints its own generated roadmap instead of excluding it. That
exclusion was forced by the tool rather than chosen by the project, which is
what made it worth fixing; removing it was only possible once a wrapped body
kept its summary, so the release dogfoods both halves of what it ships.
