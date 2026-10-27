---
name: tests
description: Write focused tests for the attached code that cover its edge cases and failure paths.
---

Write tests for the attached code.

1. Identify the public behaviour worth pinning: the inputs callers actually pass and the outputs they check.
2. Write the smallest test set that would catch a real regression — happy path, boundaries, and every error branch the code can take.
3. Use the project's existing test framework and naming style. If none is visible, use the standard library of the language with plain `assert`.
4. Each test states its expectation in the name: what is arranged, what is acted on, what must be true after.
5. No mocking unless the dependency is genuinely unavailable in the test environment; say which tests need setup the reader must run.

Return tests as a complete file or a block that can be pasted in, and name the command that runs them.
