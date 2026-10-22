---
name: speckit-specify
description: >
  Stage 1 of the spec-kit loop: write the machine-checkable spec for a
  multi-step problem before any code exists. Produces spec.md with four
  sections — problem, acceptance graded by the repo's own commands,
  out-of-scope, constraints. Use when a problem card asks for a spec, when
  the user says "specify" or "write the spec", or when a multi-step problem
  arrives without machine-checkable acceptance criteria.
---

# Speckit Specify

Write `spec.md` at the path the card names (default: worktree root). One
page maximum, four sections exactly:

## Problem
One paragraph. What is broken or missing, and for whom.

## Acceptance
Machine-checkable criteria ONLY. Each criterion names the repo's own
command that grades it — `cargo test -p <crate> --all-features`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`,
`cargo check --workspace`. A criterion with no grading command is not an
acceptance criterion; put it under Needs clarification instead.

## Out of scope
What this work explicitly does not touch.

## Constraints
Tech choices the implementation must respect: existing crates only, no new
dependency unless the card grants it by name, no new crate, no new CLI
group.

## Needs clarification
Anything only a human can settle. Ask; do not guess.

Rules:
- If the problem is single-step (one line, one test), say so in spec.md
  and STOP — the loop is for multi-step problems; a direct run is cheaper.
- Do not write code at this stage.
- Do not run the grading commands yet — that is simulate/implement's job.
