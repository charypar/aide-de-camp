---
name: review-walkthrough
description: Prepare a narrated walkthrough of the current workspace diff for a human reviewer.
---

Prepare a thorough, code-led walkthrough of the captured changes. Start with a concise level-one title describing the concrete change, followed by a short orientation, then explain the mechanics in a useful reading order. Follow concrete execution paths: inputs, key types and methods, state changes, outputs, and consequential error or recovery branches. Show the relevant code beside each explanation, including the important conditions and calls rather than only signatures. Connect consequences and tradeoffs to what the code actually does; distinguish evidence from inference. Inspect surrounding repository code where needed. Respect repository instructions. Do not modify files or act on feedback.

Cover the majority of significant changes, including behavior, persistence, protocol boundaries, and tests where relevant. Do not stop after an architectural summary or a handful of representative snippets. For addition-heavy diffs, split new files into coherent, accurately numbered excerpts and walk through their important mechanics; a list of new files and their responsibilities is insufficient. Group routine boilerplate, generated files, and lockfile churn briefly, and identify significant areas you have not covered. Prefer useful depth over an exhaustive file dump.

Use existing code identifiers and ordinary language. Avoid inventing terminology. If a new term is useful, define it near first use with Markdown definition-list syntax, explaining what it means in this code and why it matters:

Review snapshot
: The captured source and diff used for this review. Its line references remain fixed while this command runs.

Return only Markdown headings, prose, definition lists where needed, and fenced code/diff snippets. Do not generate HTML. Prefer small complete unified-diff hunks copied verbatim from the supplied diff; use current-code excerpts for long new files. Do not truncate or fabricate source to shorten a snippet.

Every snippet must have a language and JSON metadata in its fence header:

````text
```rust {"file":"src/main.rs","diff":true}
@@ -1,2 +1,2 @@
-old exact source
+new exact source
 unchanged exact source
```
````

For unchanged/current code use a starting line instead:
` ```rust {"file":"src/main.rs","start":12} `

Paths are repository-relative. Diff bodies begin with @@ hunk headers and contain only hunk lines (space, +, -, or the no-newline marker). Copy exact source text: do not add ellipses, omit diff prefixes, or invent line numbers. The tool verifies the mapping against the review snapshot; inaccurate snippets cannot be selected as source references. Explain binary files and unsupported changes in prose.

The conversation afterward is advisory: answer the reviewer's questions, inspect surrounding code, and distinguish observed facts from inference. Explicitly recorded user feedback is exported separately with no AI rewriting.
