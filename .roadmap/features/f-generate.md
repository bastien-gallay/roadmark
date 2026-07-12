+++
id = "F-generate"
type = "feature"
class = "differentiator"
effort = "L"
area = ["core", "cli"]
horizon = "shipped"
status = "done"
target = ["v0.1"]
shipped = { version = "v0.1.0", date = "2026-07-11" }
shipped_order = 1
+++

`roadmark generate` renders a deterministic `ROADMAP.md` from the `.roadmap/` tree.

The pure core (`split_frontmatter` → `parse_feature` → `sort_features` →
`render`) is string-in/string-out so it stays snapshot-testable; only
`load_config` / `load_features` touch the filesystem.
