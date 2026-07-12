+++
id = "F-validate"
type = "feature"
class = "enabler"
effort = "M"
area = ["core", "cli"]
horizon = "shipped"
status = "done"
target = ["v0.1"]
shipped = { version = "v0.1.0", date = "2026-07-11" }
shipped_order = 2
+++

`roadmap validate` reports schema errors, duplicate ids, anchor collisions and anchor drift — all issues at once, read-only.

Silent-passes when `.roadmap/` is absent so the same recipe runs on
checkouts without the source tree (CI, worktrees).
