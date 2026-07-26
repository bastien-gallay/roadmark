+++
id = "F-partial-schema"
type = "feature"
class = "enabler"
effort = "M"
area = ["core"]
horizon = "next"
status = "done"
target = ["Later"]
+++

A project may leave a schema axis out of its feature files entirely, and the generated catalog reflects only the axes it actually holds.

`horizon` joins `class`/`effort`/`severity` as an optional field (a feature
without one sorts last within its bucket), and a catalog column is emitted
only when at least one feature carries a value for that axis — `ID`,
`Status` and `Summary` excepted, being the table's identity. `—` therefore
keeps one meaning: a gap in an axis this project *does* use.

This unblocks adoption for projects whose priority already lives on a
tracker board (GitHub Projects, Jira): they get the file layout without a
second home for `horizon`, and without a catalog three columns of which are
`—` on every row. `#[serde(deny_unknown_fields)]` on the frontmatter keeps a
typo'd key (`horizen = "next"`) a parse error rather than a silent "no
horizon".

Rationale and the rejected config-driven alternative:
[ADR-0002](../../docs/adr/0002-partial-schema-adoption.md). Kept `next` (not
`shipped`) until a release actually ships it.
