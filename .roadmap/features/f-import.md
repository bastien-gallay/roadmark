+++
id = "F-import"
type = "feature"
class = "differentiator"
effort = "L"
area = ["cli", "core"]
horizon = "shipped"
status = "done"
target = ["v0.7"]
shipped = { version = "v0.7.0", date = "2026-07-27" }
shipped_order = 16
+++

`roadmark import <file>` bootstraps a `.roadmap/` tree from an existing
hand-written roadmap, doing the mechanical half and naming the rest.

This is the adoption cost. Every candidate adopter already has a `ROADMAP.md` —
that is the premise of the pitch — and the tool asked them to retype it. Seventy
rows of careful transcription is *nearly* mechanical, which is exactly the shape
of task where hand-migration goes wrong silently. [F-init](#f-init) scaffolds an
empty tree; it does nothing for a project that already has a roadmap, which is
every project that would want this one.

What a table can say is derived: id, status from the glyph or the word, horizon,
area, the body from the summary cell, and the bucket from the enclosing heading
when the document is organised that way. What it cannot say splits along the
line the schema already draws — the omissible axes are written commented out
with their value set inline, and the mandatory ones get a placeholder, because a
comment there produces a file that does not parse.

That asymmetry is the design. The imported tree generates on arrival, so the
adopter sees their roadmap, and `validate` names every undecided field instead
of refusing the tree over it. Nothing is overwritten and no prose is dropped:
unattributable text lands in a leftovers file, and some of it is a good
candidate for a [F-narrative-sections](#f-narrative-sections) entry.
