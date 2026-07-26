+++
id = "F-atomic-output"
type = "fix"
severity = "major"
effort = "S"
area = ["cli", "core"]
horizon = "now"
status = "wip"
target = ["v0.7"]
+++

`generate -o/--output <path>` writes the roadmap through a temp file and a rename, so a failed run leaves the committed `ROADMAP.md` untouched.

The documented recipe `roadmark generate > ROADMAP.md` has the shell truncate the destination to zero bytes *before* the binary runs — any error emptied the roadmap and wrote nothing in its place. [F-schema-v2](#f-schema-v2)'s `deny_unknown_fields` (0.6.0) is what made it fire in practice: a tree carrying one stray frontmatter key generated fine yesterday and destroys its roadmap today, at the exact moment the user is already confused by an error they have never seen.

stdout stays the default, so `roadmark generate | diff ROADMAP.md -` and existing pipelines are unaffected.
