+++
id = "F-crlf-parsing"
type = "fix"
severity = "major"
effort = "S"
area = ["core"]
horizon = "shipped"
status = "done"
target = ["v0.1"]
shipped = { version = "v0.1.0", date = "2026-07-11" }
shipped_order = 5
+++

CRLF-authored feature files parse correctly (Windows checkouts turned `+++` fences into `+++\r` and broke `split_frontmatter`).
