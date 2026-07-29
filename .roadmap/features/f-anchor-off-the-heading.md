+++
id = "F-anchor-off-the-heading"
type = "fix"
severity = "minor"
effort = "S"
area = ["core"]
horizon = "next"
status = "done"
target = ["Later"]
+++

The 80-column rule now holds for the whole generated document, not only for
the banner that motivated it.

[F-lintable-output](#f-lintable-output) stated the rule and shipped two lines
breaking it. `## Details` wrote each id *twice* — once as the `<a id>` anchor,
once as the heading text — around a 17-column frame, so any id past 31
characters overflowed. [F-import-bullets](#f-import-bullets) derives exactly
such ids from bullet prose, so the projects most likely to hit it were the
ones v0.8.0 was written for. `import-leftovers.md` opened on a single
180-column comment.

The anchor moved onto its own line above the heading, so the id is written
once and the binding constraint became the anchor line at 67 characters
rather than the heading at 31. A slug *derived* from prose is bounded in
characters as well as in words; an id the source wrote in backticks is never
cut, because truncating it would break the references pointing at it.

What let it ship is the part worth keeping: the assertion covered the banner,
and the claim was checked against this repo's own tree — where ids are short
and there is no leftovers file. markdownlint was no help either, since MD013's
non-strict default skips a line with no space past the limit, and a doubled id
has none. The guard is now a per-line assertion over the whole document on a
deliberately long id.
