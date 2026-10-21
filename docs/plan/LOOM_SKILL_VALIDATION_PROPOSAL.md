# Loom Skill Validation + Trigger Generation — build proposal

> Status: **SHIPPED 2026-10-21** (proposal 2026-10-20, revised 2026-10-20 after a first-principles review of the two documents — see the parent analysis §12). This document is kept as the spec of record; the code it describes was written on branch `phase-a/integrate` and landed on `main` by merge `37baf41`. §4 carries the per-criterion shipped state.
> Owner crate: `loom-skills`. Cost: **3–4 days**, no LLM spend, no new crate, no new CLI group, no new command.
> Parent analysis: [LOOM_HARNESS2_ANALYSIS.md](../LOOM_HARNESS2_ANALYSIS.md) §6.2.1, §7 Phase A, and §12 (the review that reshaped this proposal). This is the *only* slice that analysis recommends building.
> Upstream reference: `google-research/harness2` v0.1.0 `harness2/core/tree.py:265-300` (`_check_skills`), `:192-213` (`autotrigger`), Apache-2.0. **The rules are copied; the extraction is not** — upstream's trigger-text regex is broken for folded YAML (§2).

---

## 1. The problem

**Loom can install a skill that the agent will silently ignore, and reports success at every step.**

The chain, as it exists today:

| Step | Code | What it checks |
|---|---|---|
| install | `crates/loom-cli/src/commands/skills.rs:161-176` | the path exists. The file is copied. **The content is never read.** |
| index | `crates/loom-skills/src/loader.rs:44-104` | flat `*.md` files, keyed by name; hash = size + mtime |
| sync to a tool | `crates/loom-adapters/src/lib.rs:227-273` (`sync_skills_to_native`) | the directory contains a `SKILL.md`. **The frontmatter is never parsed.** |
| sync to a tool (the other path) | `crates/loom-cli/src/commands/exec.rs:518-560` (`inject_shared_skills`) | the extension is `.md`. **The frontmatter is never parsed.** |
| report | `loom skills list`, `loom scan` | counts directories. **Success.** |

Meanwhile the agent decides whether to read a skill by its frontmatter. A skill with no `description:` is not indexed by the agent and is never invoked. Upstream calls this out in exactly these words (`harness2/core/tree.py:272-276`):

> `needs a 'description:' in its frontmatter, or the skill is dropped SILENTLY at load`

and, for a `name:` that disagrees with the directory (`tree.py:283-291`):

> `frontmatter 'name: X' OVERRIDES the directory name at load, so every trigger line naming 'Y' stops matching`

and, for a skill the always-on text never mentions (`tree.py:292-297`):

> `an untriggered skill is dead weight`

**The measured state of a real installation.** A scan of the live `~/.loom/skills/` on this machine (2026-10-20) found **17 skill files** — 11 directories (3 of them symlinks to `~/.skills/*`) and 6 flat `.md` files — resolving to **11 distinct skills** (the 6 flat files are byte-identical copies of the 6 corresponding `SKILL.md` files, so they are the same skills in the wrong layout):

| Finding | Count | Detail |
|---|---|---|
| skill with **no frontmatter at all** | **1** | `~/.loom/skills/gdrive/SKILL.md` — the file begins `# Google Drive`, with no `---` block at all |
| skill with frontmatter but **no `description:`** | 0 | — |
| skill with **no `name:` key** | 1 | `gdrive` (same file) — the directory name is used, which is legal; this is *not* a mismatch |
| `name:` present and **disagreeing** with the directory/file stem | **0** | — |
| `description:` styles in use | 3 | YAML folded (`>`) ×12, literal (`\|`) ×1, double-quoted ×3 — **all valid YAML, all different** |
| flat `.md` skills (no directory, no `SKILL.md`) | 6 | `ponytail.md`, `ponytail-audit.md`, … — a **second, incompatible layout** |

So the bug is real and present, but it is **not** a "most skills are broken" emergency: **one of 17 files** is defective. The value of this proposal is therefore stated honestly:

- the defect rate in a curated install is low, so this is **not** a large-scale cleanup;
- but the failure is **silent by construction** — there is no path by which a user learns their skill was ignored, and the *next* install can be just as broken;
- and there is a **second layout** in the tree (flat `.md` vs `<name>/SKILL.md`) that two different sync paths handle differently, which is a latent inconsistency independent of frontmatter.

### 1.1 Three more failures the same audit found — worse than the missing `description:`

These are not upstream-derived. They are Loom's own, and they are *user-visible today*:

1. **`loom skills install` cannot install the layout-A form.** `crates/loom-cli/src/commands/skills.rs:161-176` derives the name from `src.file_stem()` and calls `std::fs::copy(src, &dst)`. Copying a *directory* is an error in `std` — verified: `std::fs::copy` on a directory returns `"the source path is neither a regular file nor a symlink to a regular file"`. So the only install path in Loom **cannot install `<name>/SKILL.md` at all** — and 11 of the 17 live skills are in that form.
2. **`loom skills remove` cannot remove a layout-A skill.** `skills.rs:177-186` calls `std::fs::remove_file` on the skill path; on a directory that returns `"Operation not permitted (os error 1)"` (also verified against `std`) and the skill stays on disk.
3. **Three skill inventories exist and all three disagree.** The live tree holds 17 files, and:

   | Inventory | Count | Populated by |
   |---|---|---|
   | `~/.loom/.cache/snapshot.db` (`skills` table) | 11 | `crates/loom-discovery/src/initializer.rs:56-70` — walks *directories* with `SKILL.md` |
   | `~/.loom/skills/skills.db` (the `SkillRegistry`) | **0** | nothing — `registry.upsert` is only called from `crates/loom-adapters/src/lib.rs:356`, which is the **MCP** import path, not skills |
   | `~/.loom/skills/skills.lock` | **0** | `loom skills install` → `update_lock` → `registry().list()` → the empty registry |

   The consequence is that **`loom skills list` returns `[]`** and `loom skills show ponytail` returns `{"found": false}` on a machine with 11 installed skills. `loom status` reports 11 because it counts the snapshot, not the registry.

**The root cause is the same in all three:** the filesystem is the only place skills exist, and nothing reads the filesystem into the registry. The validator in §3 is *not* the fix for that — it is a **different** fix, on a different axis, for a defect that occurs once in 17 files. This proposal therefore has to be read as two things, and they must be sequenced:

- **A0 — make the inventory real** (the live, user-visible bug): one filesystem walk that populates the registry, so `skills list`, `skills show`, and `skills lock` stop being blind. *Owner: `loom-skills`.*
- **A1 — validate what the walk finds** (the preventive fix): reject a skill the agent would silently ignore. *Owner: `loom-skills`.*

A0 is what makes the rest of the system honest. A1 is what stops the next broken skill from arriving. **Doing A1 without A0 means validating skills that `loom skills list` still cannot show.**

**This is a 3–4 day fix, not a 3–4 week feature.** The scope is: read the skills the filesystem already has, reject what cannot work, and generate the trigger lines Loom currently does not generate.

---

## 2. What upstream gives us (and what it does not)

Upstream validates a *harness tree* — a directory of files that Loom does not have. Loom has no always-on prompt surface, no `skills/<name>/SKILL.md` tree inside a surface, and no trigger text to validate against. So upstream's validator cannot be ported; its **rules** can.

| Upstream rule | Portable to Loom? | Why |
|---|---|---|
| skill must have a non-empty `description:` | **yes** | the agent indexes on it; without it the skill is invisible |
| `name:` must match the directory | **yes, with a decision** | see §3.1 — upstream itself allows an override mode (`must_match` vs `overrides_dir`), and Loom's live tree has **0** real mismatches, so this rule is preventive, not corrective |
| skill must be *named* in the always-on text | **no** | Loom has no always-on text. This is where the **trigger generator** replaces the rule. |
| `mode: subagent` in subagent frontmatter | not now | Loom has no subagent surface (`SubAgentConfig` in `loom-core/src/types.rs:769` is a phase→agent mapping, not a file surface) |
| scripts referenced by a skill must exist | **yes, cheap** | a skill that calls a missing script is broken in the same silent way — and `open-design/SKILL.md:156-160` references `$SKILL_DIR/scripts/*.mjs` that **do not exist** in the skill directory |

**Upstream's *implementation* of the trigger rule is itself broken, and must not be copied.** `autotrigger` (`harness2/core/tree.py:192-213`) extracts the cue with a regex:

```python
m = re.search(r"^description:\s*(.+)$", fm, re.M)
desc = m.group(1).strip().rstrip(".")
```

On a folded description this captures the literal `>`; on a literal block, the literal `|`; on a quoted scalar, the quotes. Verified by running the regex against the four styles:

| `description:` style | upstream's extracted cue |
|---|---|
| `description: >` (folded) | `>` |
| `description: \|` (literal) | `\|` |
| `description: "…"` (quoted) | `"…"` (quotes retained) |
| `description: Use when …` (plain) | `Use when …` ✅ |

**13 of 16 live descriptions in `~/.loom/skills/` are folded or literal**, so upstream's trigger text would be `- When >, load the \`ponytail\` skill.` for most of the corpus. The *rule* is portable; the *extraction* is not — a real YAML parse is mandatory (see §3.2).

Upstream's own implementation is a plain function appending to a list of error strings (`tree.py:265-300`) — **not** a rules engine, not a schema, not a plugin. That is the shape this proposal follows.

---

## 3. Design

### 3.1 Two layouts, one validator

Loom has two skill layouts in the same directory:

```
~/.loom/skills/
├── ponytail/            # layout A: <name>/SKILL.md   — synced by sync_skills_to_native
│   └── SKILL.md
├── ponytail.md          # layout B: flat <name>.md     — synced by inject_shared_skills
```

- `sync_skills_to_native` (`crates/loom-adapters/src/lib.rs:227`) only processes **directories** with `SKILL.md` → it syncs layout A.
- `inject_shared_skills` (`crates/loom-cli/src/commands/exec.rs:518`) only processes **flat `.md` files** → it syncs layout B.

Both are live, and the live `~/.opencode/skills/` shows **both** layouts side by side (symlinks to `ponytail` and to `ponytail.md` alike). The validator must therefore accept both, and report the *effective* name for each:

| Layout | Name source | Directory check |
|---|---|---|
| A: `<dir>/SKILL.md` | frontmatter `name:`, falling back to the directory name | frontmatter `name:` must equal the directory name |
| B: `<file>.md` | frontmatter `name:`, falling back to the file stem | no directory — the file stem is the name; a frontmatter `name:` that differs is an **override** (warn, not error) |

**Decision (proposed):** layout A with a mismatched `name:` is an **error**; layout B with a mismatched `name:` is a **warning**. Rationale: in layout A the directory name is the addressing key for every consumer (`sync_skills_to_native` names the symlink after the directory, `skills.db` keys on the name, the trigger line must name it); in layout B the file stem is the addressing key and there is no directory to contradict. This mirrors upstream, where `must_match` and `overrides_dir` are two distinct modes.

**The duplicate-name problem is real and measurable.** The 6 flat `.md` files are byte-identical to the 6 `SKILL.md` files of the same name, so the tree contains **6 skills under two effective names**. A validator that keys on the effective name must report these as `duplicate_name` — otherwise the trigger set would emit 12 lines for 6 skills, and `sync_skills_to_native` + `inject_shared_skills` would put both forms into a tool's native dir (which `~/.opencode/skills/` already shows: `ponytail` and `ponytail.md` side by side). Two legal resolutions, and the validator should not pick:

- **Warn** — the two forms may be intentional (one for a dir-native tool, one for a file-native tool).
- **Error at install** — the canonical form is layout A; a flat copy is a re-install artefact, and `loom skills install` should not create one.

The validator's job is to *name* the collision, not to resolve it. Resolution is a user decision and belongs with A0's layout decision (§1.1).

### 3.2 The validator

Same module as the walk — `crates/loom-skills/src/scan.rs` (one traversal, one parse per file; §3.4 explains why they must not be separate):

```rust
use std::path::{Path, PathBuf};

/// One defect in one skill. `severity` decides whether the skill is rejected
/// or merely reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    /// The skill cannot work: it will be ignored by the agent.
    Fatal,
    /// The skill can work, but something is inconsistent.
    Warn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefect {
    pub path: PathBuf,
    pub severity: Severity,
    /// Stable machine-readable code, e.g. "missing_description".
    pub code: &'static str,
    /// Human-readable reason, naming the file and the missing field.
    pub message: String,
}

/// Validate every skill in `skills_dir` (both layouts).
/// Returns *all* defects, not just the first.
pub fn validate_skill_tree(skills_dir: &Path) -> Vec<SkillDefect>;

/// Validate one skill file. `dir_name` is the expected name for layout A,
/// `None` for layout B.
pub fn validate_skill_file(path: &Path, dir_name: Option<&str>) -> Vec<SkillDefect>;
```

**Rules** (each maps to one `code`):

| code | severity | condition |
|---|---|---|
| `missing_description` | Fatal | no `description:` key, or its value is empty/whitespace |
| `missing_name` | Warn | no `name:` key — the directory/file stem is used instead |
| `name_mismatch` | Fatal (layout A) / Warn (layout B) | `name:` present and different from the expected name |
| `invalid_frontmatter` | Fatal | the YAML block does not parse, or the `---` fence is unterminated |
| `empty_body` | Warn | frontmatter parses but the file has no content after it |
| `missing_script` | Fatal | the body references `scripts/<name>` that is not in the skill's directory |
| `duplicate_name` | Warn | two skills resolve to the same effective name (the live corpus: 6 flat/dir pairs) |

**Day-one severity.** Only `missing_description` and `invalid_frontmatter` are Fatal on day one. `name_mismatch` and `duplicate_name` are reported as Warn until the validator has run against a real corpus — a validator that starts refusing skills on day one is the risk in §6. Widen the Fatal set once the count in §7 is known and the false-positive rate is zero. (Measured baseline: the live corpus has 1 Fatal, 0 real `name_mismatch`, 6 `duplicate_name` — so widening `name_mismatch` to Fatal would change nothing today, which is exactly why it stays a Warn.)

**Note on `description:` parsing.** All three YAML styles in use today (`>`, `|`, quoted) must produce the same result. A hand-rolled parser that only greps `^description:` would **break the `>` and `|` styles** — and 13 of the 16 real descriptions use them. This is the one place where a real YAML parse is required, not a regex.

**Two YAML hazards the corpus actually contains, beyond the block style:**

- **A quoted scalar is not the string.** `~/.agents/skills/graphflow/SKILL.md` has `name: "graphflow"` — a naive `^name:\s*(\S+)` regex yields `"graphflow"` *with quotes*, which compares unequal to `graphflow` and would report a false `name_mismatch`. A YAML parse strips the quotes correctly.
- **Extra keys are normal.** The live corpus uses `name`, `description`, `triggers`, `argument-hint`, and `license`. A parser that rejects unknown keys would refuse 3 of 16 live skills. The parser must ignore unknown keys and preserve them.

### 3.3 The frontmatter parser that already exists (and is never compiled)

`crates/loom-skills/src/neutral_renderer.rs` contains the tree's only YAML frontmatter parser:

- `SkillFrontmatter { name, description, tags, ecosystem_compatible, ecosystem_requires, version, extra }` — with `Option<String>` for `name` and `description`, and `#[serde(flatten)] extra: BTreeMap<String, serde_yaml::Value>` for unknown keys (which the corpus needs — see §3.2);
- `parse_frontmatter` (`:74`) — splits on the `---` fence and calls `serde_yaml::from_str` (`:97`).

**It is dead code, and it does not compile.** Two independent reasons:

1. `crates/loom-skills/src/lib.rs:1-7` declares `adapter, edit, hot_reload, loader, lockfile, manager, registry` — **not** `neutral_renderer`. And `crates/loom-skills/Cargo.toml` has **no `serde_yaml` dependency**, so the file has never been compiled. `cargo check -p loom-skills` passes regardless.
2. Compiled standalone (copied out with `serde` + `serde_yaml` + `anyhow` added), it fails with **two hard syntax errors**:
   - `:198` — `writer.write_str(&format!('\'{}\'', s.escape_default()))?` → *"character literal may only contain one codepoint"*;
   - `:354` — `#[test>` (a malformed attribute) → *"mismatched closing delimiter: `}`"*.

Nothing in the tree calls `NeutralRenderer` either — `grep -rn "NeutralRenderer" crates/ packages/` returns only the file itself.

`ROADMAP.md:23` (pillar 1) requires every unwired feature to be shipped or removed. This proposal **does not ship it as-is**; it takes the smaller option:

- **Delete the file** (~5 minutes: `rm crates/loom-skills/src/neutral_renderer.rs`). It is an unreferenced, non-compiling renderer. Keeping a broken renderer alive in order to reuse its parser is a worse trade than writing 30 lines of parser in `scan.rs`.
- Write the frontmatter parse in the new module the validator needs anyway, against `serde_yaml` directly.

**The alternative — repair and declare it — is ~0.5 days** (fix two syntax errors, add the dep, add `pub mod neutral_renderer;`, make `parse_frontmatter` a `pub` free fn) and is defensible if a neutral renderer is wanted for portability. It is **not** the cheaper path, and it is **not** on the critical path of this slice. If a renderer is wanted later, it can be rebuilt on top of the validator's parser, which will be correct by construction.

**What is not negotiable:** `serde_yaml` is needed either way (§5), and a real YAML parse is mandatory (§3.2) — upstream's regex extraction is broken (§2).

### 3.4 The trigger generator

Loom has no always-on text, so there is nothing to validate a skill's *presence* against — but the same problem exists in a different shape: **nothing tells the agent that a skill exists**. Upstream solves it by appending one trigger line per skill to the always-on file (`tree.py:192-213`):

```python
# (upstream, harness2/core/tree.py:192-213 — autotrigger)
# one line per skill, appended to the always-on body;
# a line that would push the body past the cap is skipped
```

New module `crates/loom-skills/src/scan.rs` — the **one** filesystem walk (A0) — plus `crates/loom-skills/src/trigger.rs`:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use loom_core::SkillEntry;

/// Which layout a skill was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// `<name>/SKILL.md`
    Dir,
    /// flat `<name>.md`
    Flat,
}

/// One line of always-on text per skill, so the agent knows the skill exists.
pub struct TriggerSet {
    /// Effective skill name -> the trigger line.
    pub lines: BTreeMap<String, String>,
    /// Skills that were skipped because the cap was reached.
    pub not_triggered: Vec<String>,
}

/// Maximum total characters of generated trigger text.
pub const TRIGGER_MAX_CHARS: usize = 4_000;

/// Build trigger lines for the given skills.
/// `cue_of` supplies the "when to use this" phrase for a skill —
/// by default the skill's `description:`.
pub fn trigger_lines(skills: &[SkillEntry], cue_of: impl Fn(&SkillEntry) -> &str)
    -> TriggerSet;
```

**Why one walk and one type.** A0 and A1 are the same walk: the walk finds the skills, the validator judges them, and the trigger generator describes them. Three functions, one traversal, one parsed frontmatter per file. If A1 were implemented as a separate traversal over the A0 walk, the tree would be read twice and the two could disagree — which is the failure mode this whole document is about.

**And why the type is `loom_core::SkillEntry`, not a new one.** The walk's output *is* a registry row: `SkillEntry` (`crates/loom-core/src/types.rs:282`) already carries name, **description**, path, content hash, tags, ecosystem flags, version and mtime, and `crates/loom-skills/src/registry.rs:1,90` already builds it from a row. The walk fills the same fields from the filesystem and hands the entry to `registry.upsert`. The two alternatives are worse: `SkillMetadata` (`crates/loom-skills/src/loader.rs:20`) has no `description` and no callers, and `SkillFrontmatter` lives in a file that does not compile. A third type for the same concept would be a fourth owner of "what a skill is". `Layout` is the only genuinely new type, and it is two variants.

The generated line for a skill named `ponytail` with a long description is:

```
- Skill `ponytail` is available: use it when the task mentions X, Y, or Z.
```

Design rules, each traceable to upstream or to a real failure mode:

1. **One line per skill, never a summary of the skill.** The description stays in `SKILL.md`; the line is a pointer.
2. **The cap is enforced by dropping, and the drop is reported.** `not_triggered` names every skill that did not fit. Upstream's `autotrigger` silently skips; the live symptom of that is upstream's own error message "an untriggered skill is dead weight". Loom should *say* it rather than drop it.
3. **Deterministic order.** `BTreeMap` — same skill set, same output, every time. A trigger set that reorders between runs makes diffs meaningless.
4. **The cue is the description, truncated.** Descriptions in the live corpus run 200–825 characters (longest: `ponytail` at 825, then `ui-ux-pro-max` at 729). The line must be a pointer, not a copy — so the cue is truncated to a first sentence or a fixed budget, and the truncation is *visible* in the line rather than silent.
5. **The cue comes from a parsed YAML value, never a regex.** Upstream's `autotrigger` regex yields the literal `>` for a folded description (§2). Loom's cue must come from the parsed scalar.

**Where the trigger text goes is a product decision, not this proposal's.** Loom currently injects nothing into an agent's prompt (`crates/loom-cli/src/commands/exec.rs:263` sets only `LOOM_AGENTS_DIR`, `LOOM_ENABLED`, `LOOM_PROJECT_NAME`). Three candidates:

| Option | Where | Cost | Risk |
|---|---|---|---|
| **A. A file, not an injection** | `~/.loom/skills/TRIGGERS.md`, referenced by the user | ~0 | the agent must be told to read it — but the *skill author* or a README can say so. Lowest risk. |
| B. Inject into the tool's native always-on file | `AGENTS.md` / `systemprompt.md` per tool | days | changes every agent's behaviour; a prompt-injection surface is a real product decision (see the analysis doc §8) |
| C. `loom exec` env var pointing at the file | `LOOM_SKILL_TRIGGERS=~/.loom/skills/TRIGGERS.md` | hours | the agent must be told to read it — same as A, but discoverable |

**Recommendation: option A or C for this slice, and defer B.** The generator produces the text; where it lands is a separate decision. A generator with no consumer is the "store with no producer" mistake in miniature — so the proposal includes a consumer: **`loom skills list` prints the generated trigger set**, and `loom skills install` writes it. That is a real, if modest, consumer, and it costs nothing.

### 3.5 Where the validator is wired in

**A0 first: one walk that populates the registry.** The registry is the system's own declared source of truth — `crates/loom-discovery/src/lib.rs:128` says *"Skills → `SkillsEngine` registry (source of truth, not filesystem)"* — and it is empty. So the walk is not a new mechanism; it is the missing half of an existing one:

| Site | Action | Rationale |
|---|---|---|
| `crates/loom-skills/src/scan.rs` (**new**) | walk `~/.loom/skills/` once, handling **both** layouts, parse each frontmatter, produce `Vec<SkillMeta>` + `Vec<SkillDefect>` | the single place skills are read |
| `crates/loom-skills/src/registry.rs` | populate from the walk: `SkillMeta` → `loom_core::SkillEntry` → `registry.upsert` | the registry gains the `description` it already has a column for (`registry.rs:35`, `NOT NULL`) |
| `crates/loom-cli/src/commands/skills.rs:161-176` (install) | accept **both** forms: a directory (copy the tree) or a file (copy the file); validate **before** copying; on a Fatal defect, refuse and name the defect | fixes the install bug in §1.1 and stops broken skills entering the tree |
| `crates/loom-cli/src/commands/skills.rs:177-186` (remove) | remove a file **or** a directory tree | fixes the remove bug in §1.1 |
| `crates/loom-adapters/src/lib.rs:227` (`sync_skills_to_native`) | validate **before** symlinking; skip a Fatal-defective skill and record it in the `SyncReport` notes | a defective skill must not reach a tool's native dir |
| `crates/loom-cli/src/commands/exec.rs:518` (`inject_shared_skills`) | same as above — this path handles layout B | the two paths must agree |
| `crates/loom-cli/src/commands/doctor.rs` | add a check row: "Skill validation" — pass / N warnings / N fatal | `loom doctor` is the existing diagnostic surface; no new command |
| `crates/loom-cli/src/commands/skills.rs` (`list`, `show`) | list from the registry, with the validation state per skill; print the generated trigger set | the validator's output must be visible somewhere — and `list`/`show` must stop returning `[]`/`{"found": false}` on a populated tree |
| `crates/loom-cli/src/commands/lock.rs` | no change needed — once the registry is populated, `lock --generate` works | it already reads `registry().list()`; it is blind because the registry is empty |

**`doctor` needs no new plumbing.** `crates/loom-cli/src/commands/doctor.rs:50` already builds a `Vec<CheckRow>` with `CheckStatus::{Pass, Fail, …}` and a `with_explanation` detail — one more row is one function call.

**A `SyncReport` note, not a hard error.** `sync_skills_to_native` returns `Result<usize>`; a hard error would abort the whole sync for one bad skill. Skipping the bad skill and reporting it in `SyncReport::notes` matches the existing pattern (the MCP branch does exactly this: `report.notes.push(format!("skills sync to {} failed: {e}", native_dir))`), and `SyncReport::notes` is printed by `loom thread` (`crates/loom-cli/src/commands/thread.rs:35`) and `loom sync` (`crates/loom-cli/src/commands/sync.rs:37-38`) — so the note reaches the user.

### 3.6 What this proposal does **not** do

- No `SurfaceSpec`, no per-tool surface directory, no `manifest.toml`, no `loom surface` command. (See the analysis doc §7 Phase D — deferred with three named preconditions.)
- No always-on prompt injection. (Option B above is explicitly deferred.)
- No experience library, no probe loop, no selection. (Analysis doc §7 Phases B–E.)
- No change to the skill *registry schema* (`skills.db`) — a `description` column already exists (`crates/loom-skills/src/registry.rs:35`, `NOT NULL`) and the registry's own type `loom_core::SkillEntry` already carries `description` (`crates/loom-core/src/types.rs:282`). A0 **populates** the existing schema; it does not extend it.
- No migration of the 6 flat `.md` skills to layout A, and no deletion of the duplicates. The validator *names* the collision; resolving it is a user decision.
- No repair of `neutral_renderer.rs` — it is deleted (§3.3), not revived.
- No change to `snapshot.db` — it is a discovery snapshot with a different job; it is not the registry.

---

## 4. Acceptance criteria

**Status (2026-10-21): all 23 shipped.** Implemented on branch `phase-a/integrate`, landed on `main` by merge `37baf41`. The per-criterion commit below is the commit that shipped it; the card commits are the A1–A5 merges, the rest are the PA-FIX commits that closed the review's findings. Every criterion was re-run against the merged tree in a `LOOM_AGENTS_DIR` sandbox holding a **copy** of the live corpus (11 distinct skills, 6 flat/dir duplicate pairs, 1 skill with no frontmatter). The review record is `docs/plan/LOOM_PHASE_A_REVIEW.md`.

**A0 — the inventory becomes real**

1. `loom skills list` on a tree with 11 skills returns **11**, not `[]` — the registry is populated from the filesystem. — shipped `88cc32b`, defect-drop fixed in `5c2059c`
2. `loom skills show <name>` finds a skill that exists on disk, in either layout. — shipped `88cc32b`
3. `loom lock --generate` writes a lock file with one entry per installed skill. — shipped `88cc32b`, lock follows install/remove in `05d7852`
4. `loom skills install <dir>` installs a `<name>/SKILL.md` directory; `loom skills install <file.md>` installs a flat file. Both succeed. *(The directory case fails today — §1.1.1.)* — shipped `54bde99`
5. `loom skills remove <name>` removes both a flat file and a directory skill, leaving nothing behind. *(The directory case fails today — §1.1.2.)* — shipped `54bde99`, lock entry dropped in `05d7852`
6. The three inventories agree: `skills.db` rows == the number of distinct skills on disk == the lock file entries. — shipped `88cc32b` + `05d7852`

**A1 — validation**

7. A skill with no `description:` is **refused at install**, with the file path and the missing field named in the error. — shipped `54bde99`, exact message asserted in `ccdf6d9`
8. A skill with a `name:` that disagrees with its directory (layout A) is refused, with both values shown. — shipped `3db533b` (rules) + `54bde99` (refusal at install)
9. A flat `.md` skill with a disagreeing `name:` is **warned about, not refused** (§3.1). — shipped `3db533b` (rules) + `54bde99` (warn at install)
10. `validate_skill_tree` returns **every** defect in the tree, not just the first — one call, one list. — shipped `3db533b`
11. All three YAML description styles in use today (`>`, `|`, double-quoted) parse to the same effective description. A regression test uses the real `ponytail/SKILL.md` shape (`description: >` with a folded multi-line value) and the real `ui-ux-pro-max` shape (a quoted value). — shipped `3db533b`, same-value test in `ccdf6d9`
12. A quoted `name:` (`name: "graphflow"`) does **not** report a false `name_mismatch`. — shipped `3db533b`
13. Unknown frontmatter keys (`triggers`, `argument-hint`, `license`) are preserved and do not make a skill invalid. — **partly shipped** `3db533b`: the second half (not invalidating) holds and is tested (`unknown_frontmatter_keys_do_not_invalidate`); the first half does **not** — unknown keys are *ignored*, not preserved (`Frontmatter` keeps six known fields). Left open deliberately; see the review's MINOR 4.
14. A skill with an unterminated `---` fence is reported as `invalid_frontmatter`, not silently treated as "no frontmatter". — shipped `3db533b`
15. A skill that references a `scripts/…` file that does not exist in its directory is reported as `missing_script` — the live `open-design` skill is the fixture. — shipped `3db533b`; severity corrected Fatal→**Warn** in `6a8d66a` (the brief's severity table) and the install no longer refuses on it in `e09bb9e`
16. The two flat/dir copies of one skill are reported as `duplicate_name`, not silently double-counted. — shipped `3db533b`, exact defect set asserted in `f053771`
17. `sync_skills_to_native` does not symlink a Fatal-defective skill into a native tool directory, and the skip is named in the `SyncReport` notes. — shipped `fa2e20e`
18. `inject_shared_skills` (the layout-B path) applies the same rule as `sync_skills_to_native` (the layout-A path). The two paths cannot disagree. — shipped `fa2e20e` (one shared predicate, `skill_link_decision`)

**Trigger generation**

19. Trigger lines are generated for every valid skill, in a deterministic order. — shipped `9291f6a`; only a *valid* skill gets a line since `736c152`
20. A skill whose trigger line would exceed `TRIGGER_MAX_CHARS` is named in `not_triggered` — never silently dropped. — shipped `9291f6a`
21. A folded description (`description: >`) yields a cue that is the **parsed text**, not the literal `>`. — shipped `9291f6a`

**Surface**

22. `loom doctor` reports the skill validation state as a row. — shipped `fa2e20e`, counts corrected in `8409b9e`
23. `cargo test -p loom-skills` covers: missing `description:`, mismatched `name:` (layout A and B), quoted `name:`, multi-line `description: >`, unterminated fence, empty tree, the cap boundary, a missing referenced script, a duplicate flat/dir pair, and a directory install + remove round-trip. — shipped `ccdf6d9` (cases renamed after the brief) + `f053771`; the round-trip case lives in package `loom`, per the brief's correction C13

---

## 5. Effort and change surface

| Item | Change | Size |
|---|---|---|
| `crates/loom-skills/Cargo.toml` | add `serde_yaml` | 1 line |
| root `Cargo.toml` | add `serde_yaml = "0.9"` to `[workspace.dependencies]` | 1 line |
| `crates/loom-skills/src/lib.rs` | declare `scan`, `trigger` | 2 lines |
| `crates/loom-skills/src/neutral_renderer.rs` | **delete** (unreferenced, non-compiling) | −373 lines |
| `crates/loom-skills/src/scan.rs` | **new** — the walk + frontmatter parse + validation | ~200 lines |
| `crates/loom-skills/src/trigger.rs` | **new** — the trigger generator | ~80 lines |
| `crates/loom-skills/src/registry.rs` | populate from the walk (`SkillMeta` → `SkillEntry` → `upsert`) | ~30 lines |
| `crates/loom-cli/src/commands/skills.rs` | install both forms; remove both forms; list/show from the registry; print the trigger set | ~80 lines |
| `crates/loom-adapters/src/lib.rs` | validate before symlink; report skips | ~25 lines |
| `crates/loom-cli/src/commands/exec.rs` | validate before symlink (layout B) | ~25 lines |
| `crates/loom-cli/src/commands/doctor.rs` | one check row | ~20 lines |
| `crates/loom-skills/src/scan.rs` (`#[cfg(test)]`) | the checks in §4 | ~180 lines |

**Total: ~600 lines net (≈1000 written, 373 deleted), 3–4 days.** One crate gains a dependency; no crate is added; no CLI group is added; no new command is added; no new table, no new directory, no new format.

The estimate moved from 2–3 days to 3–4 because A0 (populate the registry, fix install and remove) was added after the live audit showed that the surface is blind to every skill on disk — and a validator whose output cannot be *seen* is half a fix.

**Dependencies:** `serde_yaml` **0.9.34+deprecated** — **not** in the root `Cargo.toml`'s `[workspace.dependencies]`, but already in `Cargo.lock` (transitively, via `k8s-openapi`) and in the local cargo cache (`~/.cargo/registry/cache/*/serde_yaml-0.9.34+deprecated.crate`), so adding it pulls no new transitive tree. It must be added in two places: `[workspace.dependencies]` in the root `Cargo.toml` and `loom-skills`'s own `Cargo.toml`. The only other cached YAML crate is `yaml-rust` 0.4.5, which is not serde-based.

*On the deprecation:* `serde_yaml` 0.9 is marked deprecated upstream (the maintainers moved to `serde_yml` / `serde_norway`). Neither alternative is in `Cargo.lock` or the local cargo cache, so adopting one costs a fresh download and a lockfile change — **use `serde_yaml` 0.9 now** and record the upgrade as a follow-up. (`yaml-rust` 0.4.5 is also cached, but it is not serde-based and would mean hand-rolling the scalar handling that §2 shows must be correct.)

`loom-skills` already has `serde`, `serde_json`, `sha2`, `toml`, `notify`, `rusqlite`, `anyhow`, `tracing`.

---

## 6. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| **A validator that rejects skills users already rely on.** A `description:`-less skill might work in *some* tool today. | High | Fatal defects are refused only at the two **write** paths (install, sync). Existing skills on disk are not deleted or rewritten; `loom doctor` reports them. The first release **warns** on `missing_name` and `name_mismatch`, and only the `missing_description` rule is Fatal on day one. Widen later. |
| **A0 changes what `loom skills list` returns, and something downstream depends on the empty result.** `skills.list` is served by the daemon (`crates/loomd/src/ipc_handler.rs:68`) and the MCP server (`crates/loom-mcp-server/src/lib.rs:245`); both read the same registry, so both change together. | Medium | The registry already has a `description TEXT NOT NULL` column, so the rows are representable. Check the TUI's skills tab and the MCP `list_skills` tool before shipping. |
| **The two layouts get a third.** | Medium | The validator is the single place that enumerates layouts; a new layout is one match arm. |
| **`serde_yaml` on a deprecated crate.** | Low | `serde_yaml` 0.9 is deprecated upstream. It is already in `Cargo.lock` and the local cargo cache. Adopting a successor (`serde_yml` / `serde_norway`) means a fresh download and a lockfile change for a 3–4 day slice — record it as a follow-up, not a blocker. Run `cargo audit` after the change. |
| **The trigger set has no real consumer.** | Medium | §3.4 makes `loom skills list` and `loom skills install` the consumers now. Option B (real injection) is a separate, later decision. |
| **Deleting `neutral_renderer.rs` loses work someone wanted.** | Low | It has zero callers, does not compile, and is 373 lines. `git` keeps it. If a neutral renderer is wanted later, rebuild it on the validator's parser. |
| **Scope creep into the surface schema.** | Medium | §3.6 is explicit. The analysis doc's Phase D is separate and deferred. |

---

## 7. The cheap experiment that must run first

**Before writing the validator, spend 20 minutes counting the defects.** This is the same discipline the analysis doc applies to the expensive upstream method, applied to the cheap one.

1. Point the validator at 20 real skills (the live `~/.loom/skills/` has 17; take 3 more from a native tool dir).
2. Count the defects by code.
3. **If the count is 0** (or only `Warn`-level), the validator's value is in *preventing* the defect, not in fixing one — ship it anyway, but say so honestly, and consider whether the whole slice is worth 3–4 days.
4. **If the count is ≥3 Fatal**, the fix is urgent and the numbers justify the work.

The measured count on this machine is **1 Fatal** (`gdrive`, no frontmatter at all), 0 `name_mismatch`, 6 `duplicate_name`, and 1 `missing_script` (`open-design` references `$SKILL_DIR/scripts/*.mjs` that do not exist) — i.e. **the A1 fix is preventive, not corrective**.

**But A0 has a different measurement, and it is the one that matters:** on this machine, `loom skills list` returns `[]`, `loom skills show ponytail` returns `{"found": false}`, and `loom lock --generate` writes zero entries — while 11 skills are installed. **Three of the tree's own commands cannot see a single skill.** That is not a 1-in-17 defect; that is a subsystem that is off. A0 is the part of this slice with a user-visible before/after.

The honest summary: **A0 is corrective, A1 is preventive, and A0 is what a user notices.**

---

## 8. Definition of done

- [ ] `serde_yaml` added to the root `[workspace.dependencies]` and to `crates/loom-skills/Cargo.toml`.
- [ ] `crates/loom-skills/src/neutral_renderer.rs` deleted; `crates/loom-skills/src/scan.rs` added.
- [ ] The walk handles both layouts and populates `skills.db`; the three inventories agree.
- [ ] `loom skills list` returns the installed skills; `loom skills show <name>` finds them; `loom lock --generate` writes one entry per skill.
- [ ] `loom skills install` accepts a directory **and** a flat file; `loom skills remove` removes both forms.
- [ ] `validate_skill_tree` implemented, both layouts, all defect codes in §3.2.
- [ ] `trigger_lines` implemented with the cap, the `not_triggered` report, and a cue taken from the **parsed** YAML value.
- [ ] Install path refuses Fatal defects.
- [ ] Both sync paths skip Fatal defects and report them.
- [ ] `loom doctor` shows a skill-validation row.
- [ ] `loom skills list` shows per-skill validation state and the trigger set.
- [ ] `cargo test -p loom-skills` green with the cases in §4.
- [ ] `cargo check --workspace` green.
- [ ] `loom doctor` run against the live `~/.loom/` reports the `gdrive` defect by name.

**Not in scope for this slice:** the surface schema, prompt injection, the experience library, the probe loop, migrating or deleting the duplicate flat skills, and any change to the `snapshot.db` discovery snapshot.
