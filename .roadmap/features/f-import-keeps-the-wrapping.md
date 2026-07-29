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

An imported body fits the same 80-column lint the generated document does, so
an adopter who wrapped their source gets a tree they can still lint.

Reading a checkbox bullet joins its continuation lines, and `## Details`
reproduces a body verbatim — so a source wrapped at 68 and 48 columns arrived
as one 104-column line in both the feature file and the generated roadmap.
Every word was the author's and the line existed nowhere in their file, which
is what made it a defect rather than their choice: the same shape as
[F-lintable-output](#f-lintable-output) and
[F-anchor-off-the-heading](#f-anchor-off-the-heading), wearing author's
clothes.

Both halves of the split body are re-wrapped at 80. Keeping the original
breaks was not on the table — the first-sentence split does not align with
them, and it is that split which forces the recomposition — so the choice was
between one long line and a re-wrapped one, and only the second is lintable.
Nested lists and later paragraphs keep their own lines, which are still the
author's.

The rule this sharpened is worth more than the fix: the verbatim-text
exemption is about whether the project can repair the line where it wrote it,
not about who typed the words. Text the toolchain recomposed fails that test.
