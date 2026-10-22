---
name: speckit-implement
description: >
  Stage 4 of the spec-kit loop: implement a hardened spec.md in a
  worktree, graded by the spec's own acceptance commands, verdict
  verified/partial/failed where only verified closes the card. Use when a
  hardened spec exists and the card says "implement", or the user says
  "implement the spec".
---

# Speckit Implement

Input: a hardened `spec.md` (from speckit-harden).

1. Work in a worktree — the one the card names, or
   `git worktree add .worktrees/<card-slug> -b <card-slug>/<branch> main`.
   Never edit shared checkouts.

2. Implement the smallest change that meets the acceptance section.
   Constraints section is binding: existing crates only, no new
   dependency unless the card granted it by name, no new crate, no new
   CLI group.

3. Grade with the spec's own named commands — every acceptance criterion's
   command, run in the worktree. All must pass. Use a per-run
   CARGO_TARGET_DIR or accept a clean rebuild; never trust a target dir
   shared with a different source state (Phase B lesson: stale artefacts
   grade the wrong binary).

4. Verdict:
   - **verified** — every acceptance command passes. The only verdict
     that closes the card.
   - **partial** — some pass; name which failed and why.
   - **failed** — a core criterion cannot be met. Record what blocked it.

5. If a defect surfaces that the spec missed: fix the SPEC first, then
   the code. Record the lesson in the pillar-5 store
   (`loom memory retain --type lesson`). No new store.

Never weaken, delete, or bypass a test to make a command pass — that is a
spec problem, not a code problem; send it back to harden.
