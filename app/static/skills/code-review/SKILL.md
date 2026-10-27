---
name: code-review
description: Review the attached file or selection for bugs, security problems and readability, then propose a concrete fix.
---

Review the attached code as a senior engineer would.

Work in this order:

1. Read the whole thing once without commenting. Note what the code is actually responsible for.
2. List correctness bugs first: wrong conditions, off-by-one, unhandled `None`/`null`, races, resource leaks, incorrect error handling that hides a failure.
3. List security problems second: input that reaches a path, shell, query or URL unvalidated; secrets in code; missing auth or origin checks; unsafe deserialization.
4. List readability issues last, and only the ones that would cost a future reader real time.

For every finding give:

- the line or snippet it lives in,
- why it is wrong, in one sentence,
- the smallest change that fixes it.

Finish with the full corrected version of any function you changed, ready to paste. Do not restate the code back unchanged, and do not invent findings to fill a list: "no issues found in this category" is a valid line.
