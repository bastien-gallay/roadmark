+++
id = "F-bucket-sections"
type = "feature"
class = "differentiator"
effort = "M"
area = ["core"]
horizon = "next"
status = "done"
target = ["v0.7"]
+++

`split_by_bucket = true` emits one `##`-headed catalog per bucket instead of a single flat table, in the order `versions` declares.

`versions` was only ever a sort key, so a roadmap whose *shape* is its buckets — MoSCoW, quarters, release trains — flattened into one long table with a `Target` column. Nothing was lost, but the top-level structure a reader navigates by was, and the buckets had to wear a column heading that named a release axis they were not.

The section carries the bucket, so the bucket column drops out inside it — the same rule that drops a column no feature holds. It drops only where the heading carries the *whole* value, though: a multi-valued `target` keeps its cell because only the first entry picks the section, and an undeclared target keeps its because no heading can carry it. Empty buckets emit no heading, untargeted features collect in a trailing section, and `## Details` stays flat and stays one list because it is anchor-addressed and the catalog links into it.

Opt-in, because it rewrites every line of the generated file. `bucket_label` and `unbucketed_label` come with it: `versions` is a bucket order, and the vocabulary belongs to the project rather than to this binary.
