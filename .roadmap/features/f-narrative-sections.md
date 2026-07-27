+++
id = "F-narrative-sections"
type = "feature"
class = "differentiator"
effort = "M"
area = ["core"]
horizon = "next"
status = "done"
target = ["v0.7"]
+++

`sections` declares hand-written markdown files and where they land in the generated document, injected verbatim.

The generated document had four fixed parts and no room for prose that belongs to no single feature: dated triage notes, why *this* slice and not another, horizon commentary, which items are crowned and in what order. That prose is *about* the relationships between features, so no feature body holds it — and moving it to a separate file means neither file is the roadmap any more. Adopting roadmark used to mean keeping the inventory and dropping the reasoning, which is the half a reader trusts most.

Three slots, named for the document's structural landmarks rather than for individual sections, so they keep meaning when [F-bucket-sections](#f-bucket-sections) turns the catalog into several tables. Verbatim means verbatim: no parsing, no reformatting, only the framing blank lines normalised so the output does not depend on how an editor saved the file.

A declared file that is missing is a hard error — `generate` refuses outright, so a passing `validate` would promise a document the next command will not produce. Paths stay inside the roadmap root: a document assembled partly from outside the source of truth cannot be reproduced from a checkout of it.
