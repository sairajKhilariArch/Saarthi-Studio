---
name: debug
description: Trace an error or failing behaviour from the message to its most likely cause, and give the fix.
---

Debug the reported failure.

1. Ask for the exact error text, the command that produced it, and what was expected, if any of those are missing.
2. Restate the failure in one sentence: what broke, where, and under which condition.
3. List candidate causes in order of likelihood. For each, name the one observation that would confirm or kill it.
4. Pick the most likely cause and show how the evidence you have points at it.
5. Give the fix as a concrete patch — file, line or snippet — plus the one command that verifies it.

Never claim code was run or an error reproduced. If you are inferring, say so in the line where you infer.
