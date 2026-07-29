+++
id = "F-code-spans-are-literals"
type = "fix"
severity = "minor"
effort = "S"
area = ["core"]
horizon = "next"
status = "done"
target = ["Later"]
+++

The bare-reference warning reads a code span as a literal, so a filename that
happens to contain a feature id stops reading as a mention.

The warning exists to catch prose saying `F-something` that no feature
declares. Inside backticks the text is not prose: it is a path, a filename, or
a literal. A report named after a feature — the convention this project's own
docs suggest — was reported as naming an undeclared feature, so the check
asked for a feature to exist because a *file* was named after one. There was
no way to satisfy it without changing correct content, since the path is the
file's name.

Code spans are now masked once, for the link scan and the bare-token scan
alike. `rename` is untouched: it still rewrites a path reference inside
backticks. Whether it should is a separate question this does not answer —
reproduced, renaming turns `reports/F-capture-rung2.md` into a path to a file
that does not exist, and that rewrite is now unwatched on both sides. The two
behaviours are separable, so this one lands alone.

What the old rule also covered is given up on purpose: a genuine
cross-reference written in backticks no longer warns. That is the right way
round. A missed warning costs a reader one lookup; the false one cost the
adopter a defect they could not repair.
