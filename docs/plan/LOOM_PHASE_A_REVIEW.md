# Loom Phase A — Review of the five implementation branches

**Reviewer:** lead (review branch `phase-a/review`, worktree `.worktrees/phase-a-review`)
**Date:** 2026-10-21
**Base:** `main` @ `b77b5cf` ("docs: harness2 analysis + skill validation proposal")
**Verdict: FIX FIRST** — 5 of the 23 acceptance criteria fail, and the five branches do not merge as delivered.

> **Resolution (2026-10-21): FIXED.** All 5 BLOCKERs, 5 MAJORs and 7 MINORs below were closed by the PA-FIX work, which was integrated on branch `phase-a/integrate` and landed on `main` by merge `37baf41`. The findings are kept verbatim as the record of what the five branches got wrong; the branches themselves were **not** merged as delivered — only the integrated branch was. All 23 acceptance criteria of §3 were re-run against the merged tree and pass. The shipped criterion-by-criterion state is in [LOOM_SKILL_VALIDATION_PROPOSAL.md](LOOM_SKILL_VALIDATION_PROPOSAL.md) §4.

---

## 0. Scope, branches, method

All five implementation branches exist. **No branch is missing.**

| Card | Branch | HEAD | Merges onto `main` alone? | Carries A1? |
|---|---|---|---|---|
| A1 scan.rs | `phase-a/scan` | `12a559b` | clean | yes |
| A2 registry+list | `phase-a/registry` | `dfc36f5` | clean | yes (contains A1) |
| A3 install/remove | `phase-a/install-remove` | `ad02fb9` | clean | **no** |
| A4 trigger generator | `phase-a/trigger` | `89468ca` | clean | **no** |
| A5 sync gate+doctor | `phase-a/sync-gate` | `0d98876`+`7fdd2b7` | clean | yes (contains A1) |

(`phase-a/plan` also exists — the brief, not an implementation card.)

**Method actually executed:**

1. Per-branch build/test in a dedicated worktree (`.worktrees/phase-a-review`):
   `cargo test -p loom-skills`, `cargo test -p loom`, `cargo test -p loom-adapters`,
   `cargo check --workspace`, `cargo clippy -p loom-skills -p loom -p loom-adapters`.
   All green on all five branches (log: `/tmp/phase_a_suite.log`).
   *Note: the package is named `loom`, not `loom-cli` — `cargo test -p loom-cli` fails with
   "package(s) `loom-cli` not found". The review brief's command list is wrong; `loom` was used instead.*
2. The five branches were merged together in the review worktree to exercise the
   integrated behaviour, then the 23 criteria were **run** against that build using a
   copy of the live `~/.loom/skills` corpus (11 distinct skills, 6 byte-identical
   flat/dir duplicates) in a `LOOM_AGENTS_DIR` sandbox. The real `~/.loom` was never touched.
3. Targeted probes (added to the review worktree, reverted afterwards) for
   duplicate accounting, unknown keys, flat-file script references, Fatal-skill
   triggering, and the daemon's registry freshness.

---

## 1. Findings, most severe first

### BLOCKER 1 — The registry drops exactly the skill the whole card exists to surface

`crates/loom-skills/src/registry.rs:100` (branch `phase-a/registry`)

```rust
for skill in &scan.skills {
    // No description: the skill is dropped at load. Not registered.
    if skill.description.trim().is_empty() {
        continue;
    }
```

`gdrive` is one of the 11 skills on disk and its `description` is empty. It is therefore
never written to `skills.db`, never listed, never lockable, and `loom skills show gdrive`
exits 1.

Measured on the live corpus (integrated build, `LOOM_AGENTS_DIR=/tmp/loom-sandbox`):

```
loom skills list          → 10 skills (expected 11)
loom skills list --json   → count=10  skills=10  defects=11
loom skills show gdrive   → exit 1, "Skill 'gdrive' is not registered: … has no
                            `description:` in its frontmatter"
sqlite3 skills.db         → 10 rows
skills.lock               → 10 entries
distinct skills on disk   → 11
```

**Why it matters:** criteria 1 and 6 fail. The doc comment at `registry.rs:81-93`
("a row for it would be a lie") is the opposite of the agreed rule: validation
*annotates*, it never removes an entry from the inventory. The three inventories are
supposed to agree; here they are 10 / 11 / 10. A skill the user has on disk is
invisible, and the only reason they can find out is a doctor row.

**Smallest fix:** delete the `continue` at `registry.rs:100` and let the row be written
(the defect is already reported separately by the scan and joined by name in the CLI).
Update the test at `registry.rs:545` (`sync_refuses_a_skill_without_a_description`),
which currently asserts the opposite (`registry.get("gdrive").is_none()`,
`registry.count() == 1`) — it encodes the violation as intended behaviour.

---

### BLOCKER 2 — A2 and A3 shipped two independent validators and two independent YAML parsers

`crates/loom-cli/src/commands/skills.rs` on `phase-a/install-remove`:
`:31 enum Severity`, `:38 struct SkillDefect`, `:60 fn parse_frontmatter`,
`:84 fn parse_yaml_block`, `:138 fn plain_scalar`, `:164 fn validate_skill_content`.

These duplicate `loom_skills::scan::{Severity, SkillDefect}` and re-implement YAML
parsing by hand (block scalars folded/kept by string manipulation, quotes stripped
manually). The card's own comment at `skills.rs:24-26` admits it: "The Phase A validator
(card A1) has not landed yet, so the defect check below is local".

Three consequences, all measured:

1. **The branches do not merge.** A2 imports `loom_skills::scan::{Severity, SkillDefect}`
   at `skills.rs:5`; A3 declares the same names locally → `E0255: the name 'Severity' is
   defined multiple times` at `crates/loom-cli/src/commands/skills.rs:92` and `:99`.
   `git merge-tree` also shows conflicts in `crates/loom-skills/src/lib.rs`
   (`pub mod scan;` vs `pub mod trigger;`). To get the integration to compile I had to
   hand-resolve both and rename A3's local types to `LocalSeverity`/`LocalSkillDefect` in
   the review worktree. That is a merge of five *branches*, not a merge of five cards.
2. **The two validators disagree.** A3's hand-rolled parser never validates YAML syntax.
   Installing a skill with `description: [unclosed` on the A3 branch: **exit 0,
   "Installed skill 'badyaml'"**. A1's `serde_yaml` parser reports the same file as
   `invalid_frontmatter`. So the install gate accepts a file the scanner calls broken.
3. **A3's validator covers less.** `validate_skill_content` checks only
   `missing_description`, `missing_name`, `name_mismatch`. It has no `missing_script`,
   no `empty_body`, no `duplicate_name`, no `invalid_frontmatter` — so an install can
   succeed for a file the scanner calls Fatal.

**Why it matters:** this is the single most expensive finding. It is the direct cause of
the merge conflict, of the invalid-YAML hole, and of criterion 7 being satisfied by the
wrong parser.

**Smallest fix:** delete A3's `parse_frontmatter`, `parse_yaml_block`, `plain_scalar`,
`validate_skill_content`, `Severity` and `SkillDefect`; call
`loom_skills::scan::validate_skill_file(path, dir_name)` (it already exists at
`crates/loom-skills/src/scan.rs:148` and returns exactly the defect shape A3 needs).
A3 should own only copy/remove. Net: ~130 lines of A3 deleted, one import added.

---

### BLOCKER 3 — A4's trigger generator has no consumer; the feature is unreachable

`crates/loom-skills/src/trigger.rs:38` — `trigger_lines` is re-exported at
`crates/loom-skills/src/lib.rs:19` and that is all:

```
$ grep -rn "trigger_lines\|TriggerSet" crates/ --include=*.rs
crates/loom-skills/src/lib.rs:19:pub use trigger::{trigger_lines, TriggerSet, TRIGGER_MAX_CHARS};
```

`loom skills list` does not print the trigger set. Proposal §3.4 and the brief
(line 432: "`loom skills list` prints the generated trigger set") require it, and it is
the only reason a trigger line is worth generating.

**Why it matters:** criterion 19's "generated for every valid skill" is untestable as
shipped, because nothing generates them for the user. The generator also has no notion
of validity (see MAJOR 4).

**Smallest fix:** in A2's list handler, after printing the skill list, call
`trigger_lines(&skills, |s| &s.description)` and print the lines plus `not_triggered`.
The placement decision (write `~/.loom/skills/TRIGGERS.md` vs env var) can stay deferred
for a *file*, but printing it in `skills list` is not optional.

---

### BLOCKER 4 — The daemon never re-syncs; `skills.list` over the socket is permanently stale

`crates/loom-skills/src/registry.rs:78` is the **only** call site of `sync_from_disk`
in the entire tree, and it lives inside `SkillRegistry::new`. The daemon builds one
`SkillsEngine` at `crates/loomd/src/lib.rs:58` and never rebuilds it.
`crates/loomd/src/ipc_handler.rs:68-71` (`skills.list`) and `:72-86` (`skills.show`) are
unchanged on every branch.

Measured against a real `loomd` over its Unix socket:

```
daemon started with 1 skill        → skills.list count = 1 ['ponytail']
new skill 'newskill' added on disk → skills.list count = 1 ['ponytail']   ← never re-synced
```

**Why it matters:** the CLI path re-reads the registry on every invocation, so the CLI
sees new skills and the daemon does not. Any agent talking to the daemon gets a
permanently stale inventory — and the registry is exactly the thing Phase A exists to
populate.

**Smallest fix:** call `engine.registry().sync_from_disk()` in the `skills.list` and
`skills.show` handlers in `crates/loomd/src/ipc_handler.rs` (one line each), or have
those handlers use `registry().last_scan()` refreshed on demand.

---

### BLOCKER 5 — `update_lock` only ever adds; removed skills stay in the lock file

`crates/loom-cli/src/commands/skills.rs:7-13` — identical body on A2 and A3:

```rust
let mut lock = SkillsLock::load(&lock_path)?.unwrap_or_default();
for skill in &registry_skills {
    lock.lock_skill(skill);
}
lock.save(&lock_path)?;
```

`SkillsLock::unlock_skill` exists at `crates/loom-skills/src/lockfile.rs:90` and is
**never called anywhere in the tree**.

Measured round trip:

```
loom skills install …/alpha  → "Installed skill 'alpha'"
loom skills remove alpha     → "Removed skill 'alpha'"
cat skills.lock              → [skills.alpha]      ← still there
loom skills list             → "No skills found."
```

**Why it matters:** criterion 5 ("leaving nothing behind") fails. A stale lock entry
also makes `loom skills list --locked` report the removed skill as
"in lock but removed from registry", i.e. the lock starts lying the moment anything is
removed.

**Smallest fix:** in the remove path, `lock.unlock_skill(name)` before `lock.save()`.

---

### MAJOR 1 — `missing_script` is Fatal; the brief says Warn on day one

`crates/loom-skills/src/scan.rs:294` (branch `phase-a/scan`)

The brief's severity table (C4, line 158) and line 372 are explicit:
"`missing_script` is a **Warn** on day one (C4) — it must not block an install."
A1 made it Fatal, and its test `missing_script_is_fatal` (`scan.rs:617`) locks that in.

The live `open-design` skill genuinely has no `scripts/` directory (only `SKILL.md`), so
the three detections are true positives — but with Fatal severity they make a real,
working skill uninstallable, unlinkable to native tools, and force `loom doctor` to FAIL.
The live corpus should not be able to drive the severity table.

**Smallest fix:** `Severity::Warn` at `scan.rs:294`; change the test at `scan.rs:617`
to assert `Severity::Warn` (the brief names the expected test `missing_script_is_reported`).

---

### MAJOR 2 — A flat file's `scripts/…` reference is checked against the skills directory

`crates/loom-skills/src/scan.rs:289` — `let root = path.parent().unwrap_or(Path::new("."));`

For a flat skill (`skills/foo.md`) the parent is the skills directory itself, so a
`scripts/thing.sh` mention inside a flat file is resolved as `skills/scripts/thing.sh`.

Measured (probe, reverted): a flat file `flat.md` mentioning `scripts/thing.sh` is
reported as **Fatal `missing_script`**.

The brief (line 265) is explicit: "A skill in layout B has no directory — a
`scripts/…` reference in a flat file has no directory to check against, so it is **not**
a `missing_script`." This is a false positive on the live corpus shape.

**Smallest fix:** only run the script-existence check for `Layout::Dir`.

---

### MAJOR 3 — A2 left `install` and `remove` as main's broken code

`crates/loom-cli/src/commands/skills.rs` on `phase-a/registry`:
`:156 SkillsAction::Install` → `:168 std::fs::copy(src, &dst)?`
`:172 SkillsAction::Remove` → `:175 std::fs::remove_file(&path)?`

Both handlers are byte-identical to `main`. On the A2 branch alone:

```
loom skills install …/src/alpha   → exit 1, "the source path is neither a regular file
                                     nor a symlink to a regular file"   (it was a directory)
```

and `remove` on a directory-layout skill calls `remove_file` on a directory → fails and
leaves the directory behind. This is the §12.4 defect ("`std::fs::copy` on a dir fails
+ `std::fs::remove_file` on a dir leaves it behind") that the card was supposed to close;
the brief (C9) says A2 must rewrite **four** handlers in this file, not two.

A3 does fix both — but as delivered, A2 is a branch on which `loom skills install` of a
skill directory does not work.

**Smallest fix:** A3's `install_skill`/`remove_skill` are the fix; A2 should not have
shipped a branch that leaves the old handlers in place. Merge A3's version of the file
and drop the duplicate handlers from A2.

---

### MAJOR 4 — A Fatal-defective skill still gets a trigger line

`crates/loom-skills/src/trigger.rs` has no notion of validity. Measured (probe, reverted):
a skill with a **Fatal** `name_mismatch` (layout A) and a valid description still
receives a trigger line (`skills=["totally-different"] lines=["totally-different"]`).
On the live corpus the trigger set is 10 lines including `open-design`, which carries
three Fatal `missing_script` defects.

The brief (lines 431, 443): "Only valid skills get a line. A skill with a Fatal defect is
not triggered (it cannot work). A skill with only Warn defects still is."

**Why it matters:** the trigger set is an advertisement — it tells an agent a skill
exists and is usable. Advertising a skill the validator just called Fatal is the exact
failure Phase A is meant to prevent.

**Smallest fix:** `trigger_lines` should receive the defect set and skip Fatal skills
(it already has a `Vec<String>` for `not_triggered`, so a Fatal skill can be named
there instead of silently vanishing). A signature change is enough:
`trigger_lines(&skills, &defects, cue_of)`.

---

### MAJOR 5 — A3 never registers the installed skill

`crates/loom-cli/src/commands/skills.rs` on `phase-a/install-remove` — the Install
handler (`:535`) calls `install_skill` then `update_lock`; there is **no `upsert` call
anywhere in the file**.

Measured on the A3 branch alone: install a directory skill → "Installed skill 'alpha'",
exit 0, file lands at `skills/alpha/SKILL.md` — then `sqlite3 skills.db "select name from
skills;"` → **0 rows**, and `skills.lock` → `version = 1` + `[skills]` (no entries),
because `update_lock` reads a registry that A3 never populates.

**Why it matters:** on the A3 branch the persistent inventory never learns about the
installed skill, so criterion 6 (db rows == distinct skills on disk == lock entries)
cannot hold for anything A3 installs.

**Smallest fix:** after the copy, upsert the `SkillEntry` for the installed file into
the registry, then write the lock. (`loom_core::SkillEntry` is already what
`loom-skills` uses; no new type needed.)

---

### MINOR 1 — `update_lock` is duplicated verbatim in A2 and A3

`crates/loom-cli/src/commands/skills.rs:7` on both branches — same add-only body.
Folded into one copy when BLOCKER 2 is fixed (same file, same two branches).

### MINOR 2 — `loom-adapters` tests hand-roll a temp dir instead of using `tempfile`

`crates/loom-adapters/Cargo.toml` has **no `[dev-dependencies]` section**, so A5 wrote
its own `Scratch` helper for temp dirs. AGENTS.md: "All tests use
`tempfile::tempdir()` — never write to real filesystem paths". The A5 tests are
otherwise good (6 of them, all passing, and they cover the stale-link and
user-placed-skill cases).

**Smallest fix:** add `tempfile` as a dev-dependency of `loom-adapters` and delete the
helper. (No new *runtime* dependency — `serde_yaml` remains the only addition in Phase A.)

### MINOR 3 — doctor's row miscounts and truncates

`crates/loom-cli/src/commands/doctor.rs:299-312` — the detail reads
`"17 skill(s), 4 fatal, 7 warning"` on a corpus of 11 distinct skills: it counts every
file found (11 dir + 6 flat), not distinct skills. The `refused:`/`warned:` lists are
`.take(3)`, and `open-design`'s three `missing_script` defects fill the whole refused
list, hiding any other Fatal.

**Smallest fix:** count distinct names; group the refused/warned lists by defect code so
one skill with three identical defects occupies one line.

### MINOR 4 — unknown frontmatter keys are ignored, not preserved

`crates/loom-skills/src/scan.rs` — the test is `unknown_keys_are_ignored`; the parsed
`Frontmatter` struct (`:165`) keeps only six known fields. Criterion 13 says unknown keys
are "preserved and do not make a skill invalid". The second half holds (a skill with
`triggers:`/`argument-hint:`/`license:` produces no defect — verified). The first half
is unsatisfiable without a new struct or a schema change, both of which the brief bans.
Worth a wording fix in the criterion, not in the code.

### MINOR 5 — `scan.skills` contains both copies of a duplicate pair

`crates/loom-skills/src/scan.rs:64-135` — a flat/dir pair yields two entries with the
same name (probe: `skills.len()=2 names=["dup","dup"]`). The registry, the lock and
`loom skills list` all de-duplicate correctly (6 `ponytail*` rows, all pointing at the
directory copy), so criterion 16 holds at the inventory level — but any future consumer
that iterates `scan.skills` directly will double-count.

**Smallest fix:** de-duplicate in `scan_skill_tree` (directory layout wins, matching
`trigger.rs:63 pick_one_per_skill`), or document the invariant on the field.

### MINOR 6 — `neutral_renderer.rs` is deleted only on the branches that carry A1

`git ls-tree <branch> crates/loom-skills/src/neutral_renderer.rs` →
`scan:0, registry:0, sync-gate:0` (deleted) but `install-remove:1, trigger:1` (present).
The file is not declared as a module in any branch's `lib.rs` (main's `lib.rs` has no
`mod neutral_renderer`), so it is dead code either way — but pillar 1 is ship-or-remove,
and it should not survive the merge.

### MINOR 7 — the review brief's package names are wrong

`cargo test -p loom-cli` / `cargo clippy -p loom-cli` fail: the package is `loom`
(`crates/loom-cli/Cargo.toml` → `name = "loom"`). Every command in the review brief
except that one was run as written.

---

## 2. What is genuinely good

Stated plainly, because it is most of the work:

- **No fourth "what a skill is" struct.** `scan.rs` builds `loom_core::SkillEntry`
  directly (`scan.rs:319`). `SkillEntry` (crates/loom-core/src/types.rs:282) is reused.
  `SkillMetadata`/`SkillFrontmatter` are pre-existing on `main`, not added here.
- **Real YAML, not regex.** `scan.rs:201` uses `serde_yaml::from_str`. The folded-`>`
  cue is the parsed text (`trigger.rs` `clean_cue`/`is_yaml_artifact` reject a bare `>`
  indicator; the test `folded_description_yields_the_joined_text_never_the_indicator`
  pins it). No literal-`>` cue anywhere.
- **A quoted `name:` is not a mismatch.** `scan.rs` test `quoted_name_is_not_a_mismatch`
  uses the real `name: "graphflow"` shape.
- **All defects are returned, not just the first.** Probe: a 3-skill tree produced 5
  defects (`missing_description`, `name_mismatch`, `empty_body`, `missing_name`,
  `missing_description`) from one `validate_skill_tree` call. `every_defect_is_returned`
  also exists in-tree.
- **Over-cap skills are named, not dropped.** `trigger.rs:50-53` puts them in
  `not_triggered`; `cap_drops_the_line_and_names_the_skill` pins it.
- **`neutral_renderer.rs` was deleted, not repaired** — on the branches that carry A1.
- **No §7.1 cut-list creep on any branch.** No new crate, no new CLI group, no
  `SurfaceSpec`, no `~/.loom/surfaces/`, no `~/.loom/experience/`, no full-tree
  retention, no prompt-injection path, no `skills.db` schema change (the validation
  state is joined by name in the CLI, as intended).
- **One new dependency in total: `serde_yaml`** (root `Cargo.toml` +
  `crates/loom-skills/Cargo.toml`). `loom-adapters` gained no dependency at all.
- **A5's two sync paths share one predicate.** `sync_skills_to_native`
  (`crates/loom-adapters/src/lib.rs:237`) and `inject_shared_skills`
  (`crates/loom-cli/src/commands/exec.rs:518`) both call
  `loom_adapters::skill_link_decision` over the same `validate_skill_tree` output, and
  the test `both_sync_paths_agree_on_the_same_file` pins it. Fatal ⇒ not linked, named
  in the report notes. A stale symlink to a refused skill is removed; a native skill
  the user placed is never touched. That is correct and well tested.
- **The daemon and CLI agree at startup** (both 10), and the shared `SkillsEngine`
  construction is sound — the daemon's problem is refresh, not a divergent second code path.
- **Every branch builds clean and passes its own tests**: `cargo check --workspace` clean,
  `cargo clippy` clean, no warnings.

**No tautological test was found.** A5's `both_sync_paths_agree_on_the_same_file`
compares two independent call sites rather than asserting a mock against itself, and
`one_fatal_skill_does_not_abort_the_rest_of_the_sync` exercises a real multi-skill tree.
A2's `sync_refuses_a_skill_without_a_description` is not tautological — it is wrong
(BLOCKER 1), which is a different defect.

---

## 3. The 23 acceptance criteria

Run against the integrated build on the live corpus, plus targeted probes.
`PASS` = demonstrated by command output. `FAIL` = demonstrated to be wrong.
`UNVERIFIABLE` = no evidence available.

| # | Criterion | Verdict | Evidence |
|---|---|---|---|
| 1 | `loom skills list` on 11 skills returns 11 | **FAIL** | `loom skills list` → 10; `--json` → `count=10, skills=10, defects=11`; gdrive absent. Root cause: `registry.rs:100`. |
| 2 | `loom skills show <name>` finds a skill on disk, either layout | PASS | `show ponytail` → path `/tmp/loom-sandbox/skills/ponytail/SKILL.md` (the FILE — EISDIR bug fixed); `show flatonly` → `/tmp/flat-test/skills/flatonly.md`; `show ui-ux-pro-max` OK. (gdrive is excluded for the reason in #1/#6 — it has no frontmatter, so it is not a skill.) |
| 3 | `loom lock --generate` writes one entry per installed skill | PASS | "✓ Lock file generated at … (10 skill(s))", 10 `[skills.*]` entries — one per registry row. The 11th is missing for the reason in #6, not because lock generation is broken. |
| 4 | `install <dir>` and `install <file.md>` both succeed | PASS | integrated: dir → `skills/alpha/SKILL.md` (exit 0); flat → `skills/beta.md` (exit 0). **Caveat:** on the A2 branch alone the directory install fails (MAJOR 3). |
| 5 | `remove <name>` removes flat file and directory skill, leaving nothing behind | **FAIL** | files and `skills.db` rows removed; **the lock still holds `[skills.alpha]` and `[skills.beta]`** after remove. `update_lock` is add-only (`skills.rs:7-13`); `unlock_skill` (`lockfile.rs:90`) never called. |
| 6 | skills.db rows == distinct skills on disk == lock entries | **FAIL** | 10 == **11** == 10. `sqlite3 skills.db` → 10 rows; 11 distinct skills on disk; lock → 10 entries. |
| 7 | no `description:` → refused at install, path and field named | PASS | exit 1: "Refusing to install skill 'nodesc': /tmp/itest/src/nodesc/SKILL.md has no 'description:' in its frontmatter — the agent drops the skill silently [missing_description]" |
| 8 | layout-A `name:` disagreement → refused, both values shown | PASS | exit 1: "Refusing to install skill 'totally-different': /tmp/itest/src/mismatch/SKILL.md declares name 'totally-different' but its directory name is 'mismatch' [name_mismatch]" |
| 9 | flat `.md` `name:` disagreement → warned, not refused | PASS | exit 0: "warning: /tmp/itest/src/flatmismatch.md declares name 'other-name' but its file name is 'flatmismatch' [name_mismatch]" |
| 10 | `validate_skill_tree` returns every defect, not just the first | PASS | probe: 3-skill tree → 5 defects `["SKILL.md:missing_description","SKILL.md:name_mismatch","b.md:empty_body","b.md:missing_name","c.md:missing_description"]`; in-tree test `every_defect_is_returned` |
| 11 | `>`, `|`, double-quoted descriptions parse to the same effective text | PASS | `scan.rs` tests `folded_description_is_parsed` / `literal_description_is_parsed` / `quoted_description_is_parsed`; live corpus: ponytail `>` folded, open-design `|` literal, ui-ux-pro-max `"…"` quoted — all parse |
| 12 | quoted `name: "graphflow"` does not report a false mismatch | PASS | `scan.rs` test `quoted_name_is_not_a_mismatch`; no `name_mismatch` in the live corpus |
| 13 | unknown frontmatter keys preserved and not invalidating | PASS | dir skill with `triggers:`/`argument-hint:`/`license:` → no defect. (Keys are ignored, not preserved — see MINOR 4.) |
| 14 | unterminated `---` fence → `invalid_frontmatter` | PASS | `scan.rs:367` returns "the `---` frontmatter fence is never closed" → code `invalid_frontmatter` (:207); test `unterminated_fence_is_invalid_frontmatter` |
| 15 | missing referenced `scripts/…` reported as `missing_script` | PASS | live `open-design` (whose `scripts/` really is absent) → 3 `missing_script` defects. (Severity is Fatal, brief says Warn — MAJOR 1.) |
| 16 | flat/dir copies reported as `duplicate_name`, not double-counted | PASS | 6 `duplicate_name` warnings (ponytail.md, ponytail-audit.md, ponytail-debt.md, ponytail-gain.md, ponytail-help.md, ponytail-review.md); `skills.db` holds 6 `ponytail*` rows, all pointing at the directory copy. (`scan.skills` keeps both — MINOR 5.) |
| 17 | `sync_skills_to_native` does not symlink a Fatal-defective skill; the skip is named in the notes | PASS | `skill_link_decision` (`crates/loom-adapters/src/lib.rs:321`) → `linkable=false` + note "skipped skill '{name}': {code} — {message}"; test `fatal_skill_is_not_linked_and_is_named` |
| 18 | `inject_shared_skills` applies the same rule; the two paths cannot disagree | PASS | both call `loom_adapters::skill_link_decision` over the same `validate_skill_tree` output (`adapters/src/lib.rs:268`, `exec.rs:547`); test `both_sync_paths_agree_on_the_same_file` |
| 19 | trigger lines generated for every valid skill, deterministic order | **FAIL** | order is deterministic (BTreeMap, `output_order_is_stable`), but `open-design` — 3 Fatal `missing_script` defects — receives a line, and the generator has no validity notion (probe: a Fatal `name_mismatch` skill also got a line). Plus there is no consumer at all (BLOCKER 3). |
| 20 | an over-cap skill is named in `not_triggered`, never silently dropped | PASS | `trigger.rs:50-53`; test `cap_drops_the_line_and_names_the_skill` |
| 21 | folded `description: >` yields the parsed text, not a literal `>` | PASS | `trigger.rs` `is_yaml_artifact`/`clean_cue`; test `folded_description_yields_the_joined_text_never_the_indicator` |
| 22 | `loom doctor` reports skill validation state as a row | PASS | `loom doctor` → "Skill validation | FAIL | 17 skill(s), 4 fatal, 7 warning; refused: …/gdrive/SKILL.md (missing_description), …/open-design/SKILL.md (missing_script) …" — row present, gdrive named. (Counts truncated — MINOR 3.) |
| 23 | `cargo test -p loom-skills` covers the ten listed cases | **FAIL** | `loom-skills` covers missing `description:` (✔), layout-A/B `name:` mismatch (✔), quoted `name:` (✔), `description: >` (✔), unterminated fence (✔), empty tree (✔), cap boundary (✔), missing script (✔), duplicate flat/dir pair (✔) — but the **directory install + remove round-trip is in package `loom`** (`crates/loom-cli/src/commands/skills.rs:634`), not in `loom-skills`. |

**Tally: 18 PASS, 5 FAIL (#1, #5, #6, #19, #23), 0 UNVERIFIABLE.**

---

## 4. Verdict

**FIX FIRST.**

The five cards contain a good scanner, a good shared sync predicate, and a good trigger
generator. They do not compose: the branches conflict on merge, the registry drops the
skill the whole card exists to surface, the trigger generator is unreachable, the daemon
never refreshes, and the lock only ever grows. Five of the 23 criteria fail.

Order of work, cheapest first:

1. **A3** — delete its local `Severity`/`SkillDefect`/`parse_frontmatter`/
   `parse_yaml_block`/`plain_scalar`/`validate_skill_content` and call
   `loom_skills::scan::validate_skill_file` (BLOCKER 2). This alone unblocks the merge
   and closes the invalid-YAML hole.
2. **A1** — `missing_script` → Warn (MAJOR 1); skip the script check for flat files
   (MAJOR 2).
3. **A2** — remove the `continue` at `registry.rs:100` and fix the test at
   `registry.rs:545` (BLOCKER 1); add `lock.unlock_skill(name)` to the remove path
   (BLOCKER 5); refresh the registry in `crates/loomd/src/ipc_handler.rs` (BLOCKER 4);
   upsert on install (MAJOR 5).
4. **A4** — give `trigger_lines` the defect set so Fatal skills are not advertised
   (MAJOR 4), and wire it into `loom skills list` (BLOCKER 3).
5. **A5** — no functional change required; add `tempfile` as a dev-dependency
   (MINOR 2) and drop the hand-rolled `Scratch` helper.

After 1–4, re-run the 23 criteria. I would not merge before at least 1, 2, 3 and 4
land, because 1–3 are the difference between "the validation is visible" and "the
validation silently removed a skill from the inventory".
