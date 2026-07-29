+++
id = "F-import-keeps-the-wrapping"
type = "fix"
severity = "minor"
effort = "S"
area = ["cli", "core"]
horizon = "next"
status = "done"
target = ["Later"]
+++

An imported body wraps to the same 80 columns the generated document keeps, so
an adopter who wrapped their source gets a tree they can still lint.

Reading a checkbox bullet joins its continuation lines, and `## Details`
reproduces a body verbatim — so a source wrapped at 68 and 48 columns arrived
as one 104-column line in both the feature file and the generated roadmap.
Every word was the author's and the line existed nowhere in their file, which
is what made it a defect rather than their choice: the same shape as
[F-lintable-output](#f-lintable-output) and
[F-anchor-off-the-heading](#f-anchor-off-the-heading), wearing author's
clothes.

Both halves of the split body are re-wrapped at 80, and so is a table row's
Summary cell — a cell is one line by construction, so the table, the more
common import shape, was the likelier source of an over-wide body. Keeping the
original breaks was not on the table: the first-sentence split does not align
with them, and it is that split which forces the recomposition. Nested lists
and later paragraphs keep their own lines, which are still the author's.

The wrap never opens a line on a token that would start a markdown block.
Moving words to column 0 makes markdown read them structurally, and a greedy
wrap put a `1.` at the start of a line — CommonMark then read the author's
sentence as an ordered list, trading a line-length error for a list error and
changing the meaning on the way. Such a line overflows instead. Width is a
lint limit; meaning is not negotiable. So a paragraph carrying a `1.`, `-` or
`>` at a wrap boundary keeps one over-wide line — a stated exception, on the
same terms as a URL longer than the budget.

The rule this sharpened is worth more than the fix: the verbatim-text
exemption is about whether the project can repair the line where it wrote it,
not about who typed the words. Text the toolchain recomposed fails that test.
