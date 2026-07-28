+++
id = "F-import-bullets"
type = "feature"
class = "enabler"
effort = "M"
area = ["cli", "core"]
horizon = "shipped"
status = "done"
target = ["v0.8"]
shipped = { version = "v0.8.0", date = "2026-07-28", pr = 62 }
shipped_order = 17
+++

`import` reads a roadmap written as checkbox bullets, not only one written as
tables — the shape most repos actually have.

[F-import](#f-import) required a markdown table, so a `ROADMAP.md` organised
as `- [x]` bullets under bucket headings imported as nothing at all. Position
replaces the header inference: the checkbox is the status, the leading
backticked token is the id, the enclosing heading is the target, and the rest
— continuation lines, nested bullets, further paragraphs — is the body. The
bullet form is the richer source, since a table cell holds one line and this
holds paragraphs, so the first *sentence* becomes the catalog Summary and the
rest stays in `## Details`.

Checklists stay checklists, which is the part that needed deciding. Bullets
are read only when the document holds no feature table, and within such a
document only the ones naming an id — as soon as one bullet does, that is the
document's own convention and the rest are prose. A nested bullet stays in its
parent's body: there are no sub-features here, and promoting one would invent
an id the source never wrote.
