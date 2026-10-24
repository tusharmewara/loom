---
name: speckit-simulate
description: >
  Stage 2 of the spec-kit loop: simulate the implementation against the
  repo without writing any code. One budget-capped agent run walks the
  spec through the real tree and returns findings tagged spec-gap,
  already-exists, risk, or infeasible, each with file:line. Use when a
  spec.md exists and the card says "simulate", or the user says "simulate
  the spec" or "dry-run the spec".
---

# Speckit Simulate

Input: a `spec.md` written by speckit-specify. One run, budget-capped by
the card (default: ~20 minutes of agent time).

Walk the repo against the spec. Write NO code. Edit NO source files.
Produce `findings.md` next to `spec.md` with one entry per finding:

```
- tag: spec-gap | already-exists | risk | infeasible | needs-clarification
  at: path/to/file.rs:123
  one sentence.
```

What the tags mean:
- **spec-gap** — the spec assumes a home the repo does not have; say where
  the code would have to go.
- **already-exists** — the spec asks for something the repo already has;
  name it with file:line. Check before building.
- **risk** — meeting this criterion will break something adjacent; name
  the caller or test that breaks.
- **infeasible** — the spec cannot be met under its own constraints; say
  which constraint blocks it.
- **needs-clarification** — only a human can settle it.

House rules:
- Resolve unknowns by editing the existing artifacts (spec.md,
  findings.md), not by rerunning stages.
- Do not fix anything. Do not soften the spec. Findings only.
- If the walk finds nothing, write "no findings" — that is a result.
