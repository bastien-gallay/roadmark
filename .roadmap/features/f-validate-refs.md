+++
id = "F-validate-refs"
type = "feature"
class = "enabler"
effort = "M"
area = ["core", "cli"]
horizon = "shipped"
status = "done"
target = ["v0.7"]
shipped = { version = "v0.7.0", date = "2026-07-27" }
shipped_order = 12
+++

`validate` checks that every cross-reference in a feature body points at a
feature that exists, and grows a soft warning tier for findings that should be
named without failing the run.

Nothing asked the question a reader hits first — *does this link go anywhere?*
Anchor drift could not catch it: drift compares a fresh regen against the
committed roadmap, and the regen embeds the same dead link, so the two agree and
the check stays silent. [F-rename](#f-rename) keeps references honest when ids
change through it; the uncovered paths are the ones that don't — a deleted file,
an id mistyped by hand, a reference written before its target exists.

Two forms, two tiers. A markdown link to a feature anchor is a hard error,
because it ships a broken anchor in the published roadmap. A bare mention in
prose is a warning, because prose legitimately names things that are not
features. Matching goes through the same anchor rule the renderer uses and the
same token boundary [F-rename](#f-rename) uses, so an id never matches inside a
longer one. Code spans are masked before the link scan, so a body may quote the
syntax without tripping its own check.

The warning tier also carries the empty-body check: the body *is* the summary
field, so a feature without one renders a catalog row that links somewhere and
says nothing.
