+++
id = "F-declared-fields"
type = "feature"
class = "enabler"
effort = "L"
area = ["core"]
horizon = "now"
status = "wip"
target = ["v0.7"]
+++

A `[fields.X]` naming something roadmark does not model declares a field of the project's own, validated for shape and optionally rendered as a linked catalog column.

The schema had no home for a tracking issue, an owner, a spec URL. `shipped.pr` covers the shipping PR but not the issue a live feature is discussed in, so that fact lived in the free-text body — where nothing validated it, no column could show it, and no projection could read it. The GitHub Projects adapter needs it to locate a board item at all.

`kind` checks shape where `values` cannot enumerate a set, and `link` turns each value into a link by substituting it for a placeholder, which keeps the forge out of the binary. There is deliberately no `pattern`: roadmark carries no regex dependency, and a half-regex would be worse than none.

The cost is that `Frontmatter` can no longer use serde's `deny_unknown_fields` — it is incompatible with the flattened map arbitrary keys require. The guarantee did not go away, it moved one layer out to a check that reads the config, and it now names the declaration that would make a rejected key legal. It also flipped direction: an undeclared *frontmatter* key is the error, where an unrecognised `[fields.X]` used to be.
