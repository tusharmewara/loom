---
name: speckit-harden
description: >
  Stage 3 of the spec-kit loop: close every simulate finding into the
  spec, or delete the criterion it invalidates. A spec that cannot be
  hardened is killed with the reason recorded. Use when findings.md from
  a simulate run exists and the card says "harden", or the user says
  "harden the spec".
---

# Speckit Harden

Input: `spec.md` + `findings.md` (from speckit-simulate).

For EVERY finding, in order:

- **Close it into the spec** — rewrite the criterion or constraint so the
  finding cannot happen, or
- **Delete the criterion** the finding invalidates (say which one went
  and why in one line), or
- **Kill the card** — if the spec cannot survive its findings, stop here,
  record the reason verbatim in findings.md, and end the card as killed.
  Stopping with a documented reason is a successful outcome.

A finding is closed only when the spec text itself changed — or when it
was `needs-clarification` and a human answered it. No finding may stay
open.

Output: hardened `spec.md`, plus one closure note per finding appended to
`findings.md`:

```
closed: spec-gap #1 → acceptance now names the shared predicate in
crates/loom-skills/src/scan.rs
```

Rules:
- Do not write code at this stage.
- Do not delete an acceptance criterion merely because it is hard — only
  because it is wrong or unreachable under the constraints.
