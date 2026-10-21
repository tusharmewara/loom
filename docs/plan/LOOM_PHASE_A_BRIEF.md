# Loom Phase A — per-card implementation brief

> **Status:** plan of record for the Phase A slice (skill inventory + validation + trigger generation).
> **Derived from:** [`LOOM_SKILL_VALIDATION_PROPOSAL.md`](LOOM_SKILL_VALIDATION_PROPOSAL.md) §3–§4, and [`LOOM_HARNESS2_ANALYSIS.md`](../../LOOM_HARNESS2_ANALYSIS.md) §7 "Phase A", §7.1 cut list, §12.
> **Provenance note:** `docs/plan/LOOM_PHASE_A_BRIEF.md` did **not** exist on `main` at the time of writing (a sibling card owns it). This brief was therefore written directly from proposal §3/§4 and analysis §7/§7.1/§12, as instructed — it does not block on the sibling card. Where this brief and the proposal disagree, **this brief wins**; every disagreement is listed in *Corrections* with its evidence.
> **Verification basis:** every fact below was read out of the live tree at `main` = `b77b5cf`. Line numbers are from that commit. Anything that could not be confirmed against code or the live `~/.loom/` tree is marked **UNVERIFIED** and must not be relied on.
> **Scope of this document:** planning only. No product code ships in this card.

---

## 1. Facts (verified)

Each fact is a direct reading of the live tree, with `file:line` evidence. The eight facts the proposal asserted (a)–(h) are **F1–F8**; **F9–F22** are additional facts the brief depends on that the proposal did not state.

### F1 — The skills registry already has a `description TEXT NOT NULL` column, and `SkillEntry` is `loom_core::SkillEntry`
- `crates/loom-skills/src/registry.rs:1` — `use loom_core::SkillEntry;` (direct import, no alias).
- `crates/loom-skills/src/registry.rs:31-44` — `CREATE TABLE IF NOT EXISTS skills (...)`; **`:34` — `description TEXT NOT NULL,`**. Full column list at `:32-43`: `id INTEGER PRIMARY KEY AUTOINCREMENT`, `name TEXT UNIQUE NOT NULL`, `description TEXT NOT NULL`, `path TEXT NOT NULL`, `content_hash TEXT NOT NULL`, `tags TEXT`, `ecosystem_compatible TEXT`, `ecosystem_requires TEXT`, `version TEXT NOT NULL`, `last_modified TEXT NOT NULL`, `created_at DATETIME DEFAULT CURRENT_TIMESTAMP`, `updated_at DATETIME DEFAULT CURRENT_TIMESTAMP`. Second table `skill_content` at `:52-56`.
- `crates/loom-core/src/types.rs:282` — `pub struct SkillEntry` (derive at `:281`). Fields: `:283 name: String`, `:284 description: String`, `:285 path: String`, `:286 content_hash: String`, `:288 tags: Vec<String>`, `:290 ecosystem_compatible: Vec<String>`, `:292 ecosystem_requires: Vec<String>`, `:293 version: String`, `:294 last_modified: String`. **9 fields, no `hash`, no `mtime`, and the two ecosystem fields are `Vec<String>`, not booleans.**
- `crates/loom-skills/src/registry.rs:90` — the row→`SkillEntry` mapping. `:114 pub fn list(&self) -> Vec<SkillEntry>`, `:120 pub fn get(&self, name: &str) -> Option<SkillEntry>`, `:156 pub fn upsert(&self, mut skill: SkillEntry) -> anyhow::Result<()>`.

### F2 — The loader indexes flat `*.md` only, and `SkillMetadata` has no description
- `crates/loom-skills/src/loader.rs:44 pub fn refresh_metadata(&self)`, `:52` — `for entry in std::fs::read_dir(&skills_dir)?` (top level only, no recursion), `:57` — `if path.extension().and_then(|ext| ext.to_str()) == Some("md")`. A directory has no `.md` extension, so `<name>/SKILL.md` is never indexed.
- `crates/loom-skills/src/loader.rs:19-29` — `pub struct SkillMetadata` (derive `:18`): `:22 name: String`, `:24 path: PathBuf`, `:26 content_hash: String`, `:28 last_modified: String`. **No `description` field.**
- The loader is **dead in production**: `refresh_metadata` is called only from `loader.rs`'s own `#[cfg(test)] mod tests` (`:189`); `SkillsEngine::loader()` (`crates/loom-skills/src/lib.rs:37`) has no external callers.

### F3 — `loom skills install` cannot install a directory; `loom skills remove` cannot remove one
- `crates/loom-cli/src/commands/skills.rs:161-176` — install. `:166-170` derives the printed name from `src.file_stem()`; `:171` — `let dst = loom_dir.join("skills").join(src.file_name().unwrap());` (keeps the source file name, so a flat `foo.md` lands flat); **`:173` — `std::fs::copy(src, &dst)?`** — on a directory this returns *"the source path is neither a regular file nor a symlink to a regular file"*. There is **no `is_dir()` branch** anywhere in the install path.
- `crates/loom-cli/src/commands/skills.rs:177-186` — remove. `:178` — `let path = loom_dir.join("skills").join(name);`; **`:180` — `std::fs::remove_file(&path)?`** — on a directory this returns *"Operation not permitted (os error 1)"*. The raw name is joined, so the installed file `foo.md` is not found unless the extension is supplied.
- No frontmatter is read anywhere in `skills.rs`: there is **no frontmatter parser in the compiled tree at all** (see F8).

### F4 — `sync_skills_to_native` symlinks any directory containing `SKILL.md`, with no name or description check
- `crates/loom-adapters/src/lib.rs:227` — `pub fn sync_skills_to_native(loom_dir: &str, native_skills_dir: &str) -> Result<usize>` — returns `Result<usize>`, **not** a `SyncReport`.
- `:237` `for entry in std::fs::read_dir(&loom_skills)?`; `:243 if !loom_skill_path.is_dir() { continue; }`; `:246 if !loom_skill_path.join("SKILL.md").exists() { continue; }` — **layout A only**; flat `.md` files are skipped entirely.
- `:260` — `std::os::unix::fs::symlink(&loom_skill_path, &native_skill_path).is_ok()` — errors are silently dropped; `count += 1` only on success. There is **no `read_to_string`, no YAML, no name/description check** anywhere in the function.
- `:275` — `#[cfg(not(unix))]`, `:276` — `fn copy_dir_recursive(...)` — the non-Unix fallback; its only caller (`:266`) is itself inside a `#[cfg(not(unix))]` block, so it is **dead on macOS/Linux**.
- `:306-314` — `pub struct SyncReport { skills_imported, skills_synced, mcp_servers_imported, mcp_servers_synced, providers_imported, notes: Vec<String> }`; **`notes` is `:313`**. It is the only `SyncReport` definition in the tree.
- The report is built one level up, by the trait default `sync_to_native` (`:46`, branch at `:72-83`); that function is the only caller of `sync_skills_to_native` and is where `report.notes.push(...)` happens today.
- `crates/loom-adapters/src/lib.rs:351` — `pub fn sync_tools_dir(loom_dir: &str) -> Result<SyncReport>`; `:353` — `loom_mcp::McpManager::new(loom_dir)?`; `:356` — `registry.upsert(&entry)?` is the **MCP** registry (`McpRegistry`) on the MCP tools-dir import path — **not** the skills registry.

### F5 — `inject_shared_skills` handles only flat `.md`
- `crates/loom-cli/src/commands/exec.rs:518` — `fn inject_shared_skills(loom_dir: &str, native_skills_dir: &str) -> Result<()>` (ends `:559`).
- `:528` — `for entry in std::fs::read_dir(&shared_dir)?`; **`:531` — `if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }`** — directories are skipped, so **layout B only**.
- No frontmatter parse. `:542-546` — Unix: symlink first, `std::fs::copy` fallback; `:547-550` — non-Unix: copy. Target name = the source file name (`:534-538`); existing targets are skipped (`:539-541`).
- `:422` — the only call site (`if let Err(e) = inject_shared_skills(loom_dir, skills_dir)`). It is the **only** skills path in `exec.rs`; there is no layout-A path there.
- `:263-279` — `fn build_base_env(...)` sets `LOOM_AGENTS_DIR` (`:268`, re-set `:274`), `LOOM_ENABLED` (`:269`), `LOOM_PROJECT_NAME` (`:272`). `LOOM_SKILLS_DIR` is set separately in `setup_launch_tool` at `:305`.

### F6 — `neutral_renderer.rs`: 373 lines, zero callers, does not compile, not declared, no `serde_yaml` dependency
- `crates/loom-skills/src/neutral_renderer.rs` — `wc -l` = **373**.
- `crates/loom-skills/src/lib.rs:1-7` — `pub mod adapter; edit; hot_reload; loader; lockfile; manager; registry;`. **`neutral_renderer` is not declared.** A repo-wide grep for `neutral_renderer` outside the file itself returns 0 hits.
- `crates/loom-skills/Cargo.toml:6-16` — `[dependencies]`: `loom-core`, `anyhow`, `rusqlite`, `serde`, `serde_json`, `sha2`, `hex`, `toml`, `tracing`, `notify`. **No `serde_yaml`.** `:18-19` — `[dev-dependencies] tempfile`.
- Root `Cargo.toml` — `grep -n serde_yaml Cargo.toml` → **no match**; `grep -rn serde_yaml --include=Cargo.toml .` → **no match in any workspace manifest**.
- `grep -rn "NeutralRenderer" crates/ packages/` → 6 hits, **all inside the file itself** (`:30` struct, `:32` impl, `:243`/`:279`/`:338`/`:356` test bodies). **Zero external callers.**
- Standalone `rustc` on a copy of the file reproduces the failures: **`:198`** — `writer.write_str(&format!('\'{}\'', s.escape_default()))?;` → *"character literal may only contain one codepoint"*; **`:354`** — `#[test>` → *"mismatched closing delimiter: `}`"*. Because the module is not declared, `cargo check -p loom-skills` **passes** — the file is excluded from the build, not breaking it.
- The only YAML frontmatter parser in the tree is inside this file: `:8 pub struct SkillFrontmatter`, `:26 pub extra: BTreeMap<String, serde_yaml::Value>` (`#[serde(flatten)]` at `:25`), `:74 fn parse_frontmatter(&self, content: &str) -> anyhow::Result<(SkillFrontmatter, String)>` — **a method on `NeutralRenderer`, not a free function** — and `:102 serde_yaml::from_str(&frontmatter_yaml)?`.

### F7 — The three inventories disagree
Measured against the live `~/.loom/` tree on this machine:

| Inventory | Count | Evidence |
|---|---|---|
| `~/.loom/.cache/snapshot.db` (`skills` table) | **11** | `sqlite3 ~/.loom/.cache/snapshot.db "select count(*) from skills;"` → `11`. Populated by `crates/loom-discovery/src/initializer.rs:56-70` — a walk of *directories* containing `SKILL.md`. |
| `~/.loom/skills/skills.db` (the `SkillRegistry`) | **0** | `sqlite3 ~/.loom/skills/skills.db "select count(*) from skills;"` → `0`. **No production code path ever writes it** (see F10). |
| `~/.loom/skills/skills.lock` | **0 entries** | 22 bytes = `version = 1\n\n[skills]\n`. `crates/loom-skills/src/lockfile.rs:216-218` — `default_lock_path` → `<loom_dir>/skills/skills.lock`. |

Live CLI behaviour, confirmed with the daemon running:
- `loom skills list --json` → `{"count": 0, "skills": []}` — served by the daemon at `crates/loomd/src/ipc_handler.rs:68-71` (`self.skills.registry().list()`).
- `loom skills show ponytail --json` → `{"found": false, "skill": null}` — `crates/loomd/src/ipc_handler.rs:72-86` (`:78` — `self.skills.registry().get(name)`).
- `loom lock --generate` → **succeeds** and writes **0 entries**: `crates/loom-cli/src/commands/lock.rs:13` (`engine.registry().list()`) → `:17` (`lock.lock_skill`) → `:19` (`lock.save`). The command is **not** broken; it is blind because the registry is empty.

### F8 — Six flat `~/.loom/skills/*.md` files are byte-identical duplicates of six `<name>/SKILL.md` files
`cmp` confirms byte equality for each pair: `ponytail.md` ≡ `ponytail/SKILL.md`, and likewise for `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, `ponytail-help`, `ponytail-review`.

### F9 — Live corpus shape (measured 2026-10-20/23)
`~/.loom/skills/` holds **21 entries**:
- **11 skill directories** — 8 real (`gdrive`, `open-design`, `ponytail`, `ponytail-audit`, `ponytail-debt`, `ponytail-gain`, `ponytail-help`, `ponytail-review`) + 3 symlinks into `~/.skills/` (`tasteful-ui` → `~/.skills/tasteful-ui-skill-pkg`, `ui-craft` → `~/.skills/ui-craft-skill`, `ui-ux-pro-max` → `~/.skills/ui-ux-pro-max-skill`).
- **6 flat `.md` files** — the six `ponytail*` duplicates from F8.
- **4 non-skill files** — `skills.db`, `skills.db-shm`, `skills.db-wal`, `skills.lock`.

**11 distinct skill names** (the 6 flat files share names with 6 of the 11 directories). 17 skill *files* in total if each `SKILL.md` and each flat `.md` is counted once.

### F10 — The skills registry is never written by any production path
`grep -rn "\.upsert(" crates/ packages/` returns 43 hits. Every hit outside `loom-skills` is `loom_mcp::McpRegistry::upsert` — a **different type** (`crates/loom-mcp/src/registry.rs:76`, `crates/loom-mcp/src/pool.rs:421/447/487`, `crates/loom-cli/src/commands/init.rs:95`, `crates/loom-cli/src/commands/mcp.rs:207`, `crates/loom-adapters/src/lib.rs:356`, `crates/loom-mcp-server/src/lib.rs:313`, `crates/loom-integration-tests/tests/{ecosystem.rs,cross_tool_workflow.rs}`). `loom_skills::registry::SkillRegistry::upsert` (`:156`) is called **only from its own unit tests** (`registry.rs:330/370/371/400`).
The repo says so itself: `crates/loom-discovery/src/lib.rs:139-141` — *"The SkillRegistry (skills.db) is not populated by `loom skills install`, which only copies files. Registry is unused here."* (`:128` — *"Skills → `SkillsEngine` registry (source of truth, not filesystem)"*.)
Consequence: `skills.db` has 0 rows today and **would have 0 rows after `loom skills install` too** — `crates/loom-cli/src/commands/skills.rs:6-16` (`update_lock`) only *reads* `registry().list()` and writes the lock file; it never upserts.

### F11 — The two CLI paths disagree with each other and with the daemon
- `skills list` daemon path: `crates/loom-cli/src/commands/skills.rs:21` (`IpcRequest::new("skills.list")`).
- `skills list` fallback (daemon down): `:47-54` — reads the skills dir and pushes **every entry's file name**, directories and files alike, with no `.md` filter. On the live tree this returns **21** entries, including `skills.db`, `skills.db-shm`, `skills.db-wal`, `skills.lock`.
- `skills show` fallback: `:141-143` — `std::fs::read_to_string(loom_dir.join("skills").join(name))` — on a layout-A skill that path is a **directory** → `EISDIR`.

### F12 — `skills.db` has a `version TEXT NOT NULL` column and no live skill carries a `version:` key
`crates/loom-skills/src/registry.rs:38` — `version TEXT NOT NULL,`. `grep -l "^version:" ~/.loom/skills/*/SKILL.md` → **no matches**. The walk must therefore synthesize a version string, or every insert fails on the NOT NULL constraint (`upsert` returns `Err`, which would abort the whole scan).

### F13 — The lock file hashes the skill *file*, so `SkillEntry.path` must point at the file
`crates/loom-skills/src/lockfile.rs:74-87` — `lock_skill` uses `skill.content_hash` if non-empty, else `compute_file_hash(&skill.path)`; `:204-213` — `compute_file_hash` does `std::fs::read_to_string(path)` and returns `""` on failure. A layout-A entry whose `path` is the *directory* would silently lock an empty hash.

### F14 — `sync_skills_to_native` has exactly one caller, and it is where `SyncReport.notes` is written
`crates/loom-adapters/src/lib.rs:46` — trait default `fn sync_to_native(&self, loom_dir: &str, home: &str) -> Result<SyncReport>`; `:72` — `match sync_skills_to_native(loom_dir, native_dir) { Ok(n) => { report.skills_synced = n; ... } Err(e) => report.notes.push(...) }`. Downstream consumers of the notes: `crates/loom-cli/src/commands/sync.rs:33-38` (`all_notes.extend(report.notes)`) and `crates/loom-cli/src/commands/thread.rs:35`. The daemon also auto-syncs on `SkillsAdded` events (`crates/loomd/src/lib.rs:380-389`).

### F15 — `loom doctor` already has the row shape a skill-validation row needs
`crates/loom-cli/src/commands/doctor.rs:10-14` — `enum CheckStatus { Pass, Warn, Fail }` with `as_str` (`:16-24`); `:29-37` — `struct CheckRow { name, status, detail, explanation }`; `:38-42` — `CheckRow::new(...)`; `:44` — `fn with_explanation(...)`; `:50` — `pub async fn run(loom_dir: &str, json: bool, fix: bool, verbose: bool) -> Result<()>`; rendering via `comfy_table::Table`. Existing tests at `:527+` (`#[tokio::test]`, `tempfile::tempdir()`).

### F16 — `lock.rs` needs no change
`crates/loom-cli/src/commands/lock.rs:10-25` (`--generate`) and `:26-87` (`--verify`) both read `engine.registry().list()`. Once the registry is populated (A2), criterion 3 works unchanged. **No card touches `lock.rs`.**

### F17 — `SkillManager` and `SkillLoader` are dead; the walk supersedes both
`crates/loom-skills/src/manager.rs:15` — `pub fn install(&self, _name: &str, _content: &str) -> Result<()> { Ok(()) }`; `:19` — `pub fn remove(...) -> Result<()> { Ok(()) }` — stubs, zero callers. `SkillLoader` is dead per F2. `SkillWatcher` (`crates/loom-skills/src/hot_reload.rs:10`) is exported at `crates/loom-skills/src/lib.rs:11` but **has no callers** either.

### F18 — The daemon holds a long-lived registry instance
`crates/loomd/src/lib.rs:58` — `skills: Arc::new(loom_skills::SkillsEngine::new(loom_dir)?)`, constructed once at daemon start. `SkillRegistry::new` (`crates/loom-skills/src/registry.rs:20`) opens the DB and calls `refresh_cache()` (`:69`). A registry populated only at construction is **stale** for the daemon's whole lifetime — which is exactly why `loom skills list` (a daemon call) must reflect the tree.

### F19 — The `loom` CLI honours `LOOM_AGENTS_DIR`
`crates/loom-core/src/lib.rs:25` — `pub fn loom_dir()` → `:31 fn resolve_loom_dir(env_value)`; `Some(dir) if !dir.is_empty() => dir.to_string()`. `crates/loom-cli/src/lib.rs:19` delegates to it. Verified live: `LOOM_AGENTS_DIR=/tmp/x loom lock --generate` writes `/tmp/x/skills/skills.lock`. Every acceptance test that touches the filesystem must use this variable (or `tempfile::tempdir()`) — **never** the real `~/.loom/`.

### F20 — The `graphflow` fixture is not in `~/.loom/skills/`
`~/.agents/skills/graphflow/SKILL.md:2` — `name: "graphflow"` (a **quoted** scalar), `:3` — `description: "图谱上下文压缩、任务规划与知识图谱编排（10 个 MCP 工具）。…"`. It lives outside the Loom skills tree; it is a *shape* fixture for criterion 12, not part of the 11.

### F21 — Measured description lengths and styles
Parsed with a real YAML parse over the live corpus:

| Skill | parsed `description` length | style |
|---|---|---|
| `ponytail` | **826** (825 after `rstrip`) | folded `>` |
| `ui-ux-pro-max` | 729 | double-quoted |
| `tasteful-ui` | 699 | double-quoted |
| `open-design` | 486 (485 after `rstrip`) | literal `\|` |
| `ponytail-review` | 457 (456) | folded `>` |
| `ponytail-audit` | 400 | folded `>` |
| `ponytail-debt` | 386 | folded `>` |
| `ponytail-gain` | 311 | folded `>` |
| `ponytail-help` | 200 | folded `>` |
| `ui-craft` | 416 | double-quoted |
| `gdrive` | — | **no frontmatter at all** (`~/.loom/skills/gdrive/SKILL.md:1` is `# Google Drive`) |

Style counts over the **17 files** (flat + dir copies counted separately): folded `>` ×12, literal `|` ×1, double-quoted ×3. Over the **11 distinct skills**: 10 have a description, 1 (`gdrive`) has none.
Frontmatter keys actually in use: `name`, `description`, `triggers` (`open-design`), `argument-hint` (`ponytail`, `ui-craft`), `license` (`ponytail`).
**All 11 frontmatter `name:` values match their directory/file stem → 0 `name_mismatch` in the live corpus.**

### F22 — Live defect census (baseline for criterion 15 and for the estimate)
- **1 × `invalid_frontmatter` / `missing_description`** — `gdrive/SKILL.md` has no `---` fence at all (`grep -n '^---'` → no match).
- **0 × `name_mismatch`** (F21).
- **6 × `duplicate_name`** — the six flat/dir pairs (F8).
- **1 × `missing_script`** — `~/.loom/skills/open-design/SKILL.md:156-160` references `node "$SKILL_DIR/scripts/list-design-templates.mjs"`, `.../list-skills.mjs`, `.../list-design-systems.mjs`; `ls ~/.loom/skills/open-design/` shows **only `SKILL.md`**. No other live skill has a dangling `scripts/…` reference (`ui-ux-pro-max/scripts/search.py` **does** exist).
- **1 × `missing_name`** — `gdrive` (no `name:` key; the directory name is used, which is legal).
- Stray file: `~/.loom/skills/ui-craft/ui-craft.md` is **byte-identical** to `~/.loom/skills/ui-craft/SKILL.md` — a nested `.md` inside a layout-A directory (see **C7**).

---

## 2. Corrections

The proposal is sound in its diagnosis but wrong or self-contradictory in **15** places. Each correction states the correct value and its evidence. **C4 and C9 are the two that change what code gets written**; the rest are precision fixes that would otherwise cost a later card an hour of archaeology.

| # | Proposal / analysis says | **Correct value** | Evidence |
|---|---|---|---|
| **C1** | §3.4: "Descriptions in the live corpus run 200–825 characters (longest: `ponytail` at 825, then `ui-ux-pro-max` at 729)". Analysis §12.6: "the measured value is **729**" for the longest. | The longest parsed description is **`ponytail` at 826 chars** (825 after `rstrip`). 729 is `ui-ux-pro-max`, the **second** longest. The range is **200–826**. | F21 (parsed with a YAML parser, not a regex). |
| **C2** | §3.5, §5, analysis §12.2 cite the description column as `registry.rs:35`. | The column is at **`crates/loom-skills/src/registry.rs:34`**. `:35` is `path TEXT NOT NULL`. The `CREATE TABLE` spans `:31-44`. | F1. |
| **C3** | §5 change surface: "`crates/loom-skills/src/registry.rs` — populate from the walk (**`SkillMeta` → `SkillEntry`** → `upsert`)". | **There is no `SkillMeta` type and none may be added.** §3.4 and analysis §12.2 both forbid it. The walk emits `loom_core::SkillEntry` directly. §5's table is stale text that contradicts §3.4. | F1 + proposal §3.4 ("the walk's output *is* a registry row"). |
| **C4** | §3.2's severity table marks `missing_script` **Fatal**; §3.2's day-one paragraph names only `missing_description` + `invalid_frontmatter` as Fatal; §3.1 says a layout-A `name_mismatch` is an **error**; the day-one paragraph says `name_mismatch` stays **Warn**; §7 counts "**1 Fatal**" on a corpus that contains a `missing_script`. | **One severity table, day one:**<br>**Fatal** — `missing_description`, `invalid_frontmatter`, `name_mismatch` **in layout A only**.<br>**Warn** — `missing_name`, `name_mismatch` in layout B, `empty_body`, `missing_script`, `duplicate_name`.<br>Reasoning: criteria **7**, **8** and **14** *require* Fatal for those three, and criterion **9** *requires* Warn for a layout-B mismatch. The proposal's own risk note ("widening `name_mismatch` to Fatal would change nothing today") confirms the day-one false-positive risk is **zero** — the live corpus has **0** mismatches (F21). `missing_script` is Warn on day one because criterion 15 only requires it to be *reported*, and making it Fatal would refuse the live `open-design` skill at install with no product decision behind it. **Widen `missing_script` to Fatal later, as a separate, product-backed change.** | Criteria 7/8/9/14/15 of §4; F21; F22. |
| **C5** | §5: `serde_yaml` is "already in `Cargo.lock` (transitively, **via `k8s-openapi`**)". | `serde_yaml` is in `Cargo.lock` at `:3851` (version `0.9.34+deprecated`), but its only dependent is **`kube-client`** (→ `kube` → `kube-runtime`), pulled in by the **optional `kubernetes` feature** of `loom-runtime` (`crates/loom-runtime/Cargo.toml:22,30`). `k8s-openapi`'s dependency list does **not** contain `serde_yaml`. The practical conclusion is unchanged — `serde_yaml-0.9.34+deprecated.crate` is in the local cargo cache, so adding it pulls **no new download** and no new transitive tree — but the provenance in the brief must be right, because "optional feature" means it is **not** in the default build graph. | `Cargo.lock` package blocks; `crates/loom-runtime/Cargo.toml:22,30`. |
| **C6** | §3.3: `neutral_renderer.rs` "fails with **two** hard syntax errors: `:198` … `:354`". | The two cited errors are exactly right, but there are **four** malformed sites: the character literal at `:198` **and** the malformed `#[test>` attribute at **`:277`, `:336`, and `:354`** (rustc reports only `:354` because it is the one that breaks the delimiter). | F6 (standalone `rustc` run). |
| **C7** | §1: "**17 skill files** — 11 directories … and 6 flat `.md` files". §3.4's walk "handles **both** layouts". | A naive recursive `*.md` walk over the live tree finds **18** files, not 17: `~/.loom/skills/ui-craft/ui-craft.md` is a **nested `.md` inside a layout-A directory**, byte-identical to that directory's `SKILL.md` (F22). **Layout A must be read as `<dir>/SKILL.md` only** — one file per directory, no recursion into it. Top-level `*.md` is layout B. Nested `.md` files inside a layout-A directory are **not skills** and must not be indexed, hashed, or triggered. | F9, F22 (`find ~/.loom/skills -mindepth 2 -name "*.md"`). |
| **C8** | §1.1 and §3.5 treat `loom skills list` as returning `[]` — a single behaviour. | There are **two** behaviours today and they disagree. The daemon path returns `[]` (empty registry); the **daemon-down fallback** (`crates/loom-cli/src/commands/skills.rs:47-54`) returns **every entry name in the skills dir** — on the live tree, **21** entries including `skills.db`, `skills.db-shm`, `skills.db-wal` and `skills.lock`. Both paths must return the same 11 skills after A2, and the fallback must stop being a raw directory listing. | F11, F9. |
| **C9** | §3.5's wiring table lists `crates/loom-cli/src/commands/skills.rs` (install) and `crates/loom-cli/src/commands/skills.rs` (remove), but never mentions that **`list` and `show` also need rewriting**, nor that `show`'s fallback does `read_to_string` on a path that is a **directory** for layout A (`:141-143` → `EISDIR`). | A2 must rewrite **four** handlers in `skills.rs`, not two: `list` (`:20-114`), `show` (`:115-160`), `install` (`:161-176`), `remove` (`:177-186`). `list` and `show` must both resolve through the registry so the daemon path and the fallback agree; `show` must read `SkillEntry.path` (the **file**), never the directory (F13). | F11, F13. |
| **C10** | §3.5/§5 say the walk produces "the skill list plus the defect list" and the registry gains "the `description` it already has a column for". | The registry also has **`version TEXT NOT NULL`** (`:38`) and **no live skill carries a `version:` key** (F12). The walk must synthesize a version (e.g. `"0.0.0"`) for skills without one, or every `upsert` fails the NOT NULL constraint and the whole scan aborts. | F12. |
| **C11** | §3.6: "No change to the skill *registry schema* (`skills.db`)". §3.5 asks `loom skills list` to show "the validation state per skill". | These are compatible only if the **validation state is not stored in the registry row**. `SkillEntry` (F1) has no defect field, and adding one would be the schema change §3.6 forbids. **Resolution: `loom skills list` joins the defect list by name in the CLI** — the registry holds the inventory, `validate_skill_tree` holds the defects, and the CLI presents both. A card must not add a column. | F1 + proposal §3.6. |
| **C12** | Criterion 6: "the number of distinct skills on disk" must equal the `skills.db` row count. Criterion 7 refuses a skill with no `description:`. | A **Fatal-defective skill must still get a registry row**. `gdrive` is one of the 11 and has no description (F22). If the registry skipped Fatal skills it would hold **10** rows, and criterion 6 (11 == 11 == 11) would fail. **Resolution: validation never removes a skill from the inventory; it annotates it.** `gdrive` gets a row with `description = ""` (the column is `NOT NULL` but an empty string satisfies it). Refusal happens **only at the write paths** — install and sync (criteria 7, 17). | F1, F22 + criteria 6/7. |
| **C13** | Criterion 23: "`cargo test -p loom-skills` covers … a **directory install + remove round-trip**". | **Impossible as written.** The install/remove code lives in the CLI crate, whose **package name is `loom`**, not `loom-cli` (`crates/loom-cli/Cargo.toml:2` — `name = "loom"`); `cargo run -p loom-cli` fails with *"package(s) `loom-cli` not found"*. The round-trip test must live in the `loom` package (`crates/loom-cli/src/commands/skills.rs`, inline `#[cfg(test)] mod tests` — the crate has **no** `tests/` directory) and runs under `cargo test -p loom`. The other ten cases in criterion 23 do belong in `cargo test -p loom-skills`. | `crates/loom-cli/Cargo.toml:2`; `ls crates/loom-cli/` shows `Cargo.toml` + `src` only. |
| **C14** | Criterion 11: "A regression test uses the **real** `ponytail/SKILL.md` shape … and the **real** `ui-ux-pro-max` shape". Criterion 12 cites `name: "graphflow"` (a real skill in `~/.agents/skills`). | The fixtures must be **inline copies of the shape**, not reads of the live tree: `AGENTS.md` requires *"All tests use `tempfile::tempdir()` — never write to real filesystem paths"*, and a test that reads `~/.loom` is not hermetic and breaks on any other machine. `graphflow` (F20) is a *shape* source, not a Loom skill — copy its two frontmatter lines, do not reference the path. | `AGENTS.md` §Testing; F20. |
| **C15** | (No proposal claim — found during verification.) | **`loom lock --generate` is not broken.** An earlier reading of *"Error: Operation not permitted (os error 1)"* turned out to be a **DSH file-sandbox artifact** (writes outside the workspace are denied), not a repo defect: run outside the sandbox, `loom lock --generate` succeeds and writes the lock file with 0 entries. **No card may treat the lock command as broken**, and no card touches `lock.rs` (F16). | F7; verified with `LOOM_AGENTS_DIR=/tmp/...` and with the installed binary. |

### 2.1 A note on the "3–4 days" estimate
**The estimate holds, but only under the two decisions C4 and C11, and A2 is bigger than the proposal's "~30 lines".**

What the proposal's change surface does not include, and what the brief now charges to A2:
- the **daemon refresh** — the daemon's registry is constructed once at start (F18), so a registry populated only at construction is stale for the daemon's lifetime; criterion 1 is a *daemon* call, so `skills.list` must refresh from disk (F7, F11);
- the **`version` synthesis** (C10/F12) and the **file-vs-directory `path`** rule (F13);
- the **`list`/`show` handler rewrites** (C9) — the proposal's table only budgets install/remove;
- the **no-schema-change join** (C11) — validation state is joined in the CLI, not stored.

Two smaller additions outside the proposal's surface: the nested-`.md` rule (C7, ~1 hour) and the raw-dir fallback (C8, ~1 hour).

**Net: 3–4 days is realistic but sits at the optimistic end; plan for ~4 days.** If the team instead wants the validation state **persisted** in `skills.db` (a column + migration + backfill), add **~1 day** — that path is explicitly *out of scope* (C11, and proposal §3.6). If `missing_script` is made Fatal on day one (C4), the live `open-design` skill is refused at install and the team needs a product decision on exemption — that is a decision, not a code change, but it blocks the A3 acceptance run.

---

## 3. Card map

Five cards. Each carries: **write scope** (exact file list), **depends on**, **acceptance criteria** (verbatim from proposal §4), and **the test it must run**.

Shared rules for every card:
- Rust only. **No new crate.** **No new CLI group** (no `loom surface`, no `loom skill-*`). **No new CLI command** — only behaviour changes inside existing `loom skills`, `loom lock`, `loom doctor`, `loom sync`, `loom exec`.
- **One new external dependency is permitted in total**: `serde_yaml` (`0.9.34+deprecated`), added to the root `Cargo.toml` `[workspace.dependencies]` and to `crates/loom-skills/Cargo.toml`. It is the only sanctioned dependency change; no card may add anything else.
- **No schema change to `skills.db`** (C11). **No change to `snapshot.db`.**
- All filesystem tests use `tempfile::tempdir()` (never the real `~/.loom/`) or `LOOM_AGENTS_DIR` pointing at a temp dir (F19).
- `cargo check --workspace` and `cargo clippy --workspace` must be clean at the end of each card.

### Shared-file rule (avoids merge collisions)
`crates/loom-skills/src/lib.rs` is touched by three cards, each adding exactly one line:

| Card | Line added to `lib.rs` | When |
|---|---|---|
| A1 | `pub mod scan;` | first |
| A2 | `pub fn refresh(&self)` / the `SkillsEngine` refresh wiring | after A1 merges |
| A4 | `pub mod trigger;` | after A1 merges |

The cards are otherwise disjoint. If A4 lands before A1, A1 adds `pub mod scan;` **and** `pub mod trigger;` in one edit and A4 skips its own — **one owner of the line at a time**, decided at merge time.

---

### A1 — Scan + frontmatter parser + validator

**Purpose.** The single place skills are read: one filesystem walk over the skills dir, both layouts, one YAML parse per file, producing the skill list and the defect list.

**Write scope (exclusive).**
- `crates/loom-skills/src/scan.rs` — **new**.
- `crates/loom-skills/src/neutral_renderer.rs` — **delete** (F6; §3.3 chose delete over repair).
- `crates/loom-skills/src/lib.rs` — add `pub mod scan;` (one line).
- `crates/loom-skills/Cargo.toml` — add `serde_yaml.workspace = true` (one line).
- `Cargo.toml` (root) — add `serde_yaml = "0.9.34+deprecated"` to `[workspace.dependencies]` (one line).
- `crates/loom-skills/src/trigger.rs` — **not** in scope (A4).

**Depends on:** nothing. **A1 is the root card.**

**Public API to implement (from proposal §3.2/§3.4, canonical per analysis §12.1):**
```rust
pub enum Severity { Fatal, Warn }

pub struct SkillDefect {
    pub path: PathBuf,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
}

/// Validate every skill in `skills_dir` (both layouts).
/// Returns *all* defects, not just the first.
pub fn validate_skill_tree(skills_dir: &Path) -> Vec<SkillDefect>;

/// Validate one skill file. `dir_name` is the expected name for layout A,
/// `None` for layout B.
pub fn validate_skill_file(path: &Path, dir_name: Option<&str>) -> Vec<SkillDefect>;
```
plus the walk entry point that A2 consumes (name fixed by this brief so A2 does not re-derive it):
```rust
/// One walk, both layouts. Returns the skills found and the defects found.
pub fn scan_skills(skills_dir: &Path) -> (Vec<loom_core::SkillEntry>, Vec<SkillDefect>);
```
**Defect codes and day-one severity — exactly the C4 table:**

| code | severity (day one) | condition |
|---|---|---|
| `missing_description` | **Fatal** | no `description:` key, or the value is empty/whitespace |
| `invalid_frontmatter` | **Fatal** | the YAML block does not parse, or the `---` fence is unterminated |
| `name_mismatch` | **Fatal** in layout A / **Warn** in layout B | `name:` present and different from the expected name for that layout |
| `missing_name` | Warn | no `name:` key — the directory/file stem is used instead |
| `empty_body` | Warn | frontmatter parses but the file has no content after it |
| `missing_script` | Warn | the body references `scripts/<name>` that is not in the skill's directory |
| `duplicate_name` | Warn | two skills resolve to the same effective name |

**Rules the implementation must honour (each traced to a verified fact):**
1. **Layout A is `<dir>/SKILL.md` — one file per directory, no recursion into it** (C7). A nested `.md` inside a layout-A directory is not a skill. Layout B is a top-level `<stem>.md`.
2. **Effective name** = frontmatter `name:` if present, else the directory name (A) / file stem (B).
3. **A real YAML parse, never a regex** (criterion 11, criterion 21). A quoted scalar must lose its quotes (criterion 12). Unknown keys (`triggers`, `argument-hint`, `license`) are **preserved** and never invalidate a skill (criterion 13).
4. **An unterminated `---` fence is `invalid_frontmatter`**, not "no frontmatter" (criterion 14).
5. **`missing_script` detection**: look for `scripts/…` references in the body (the live form is `$SKILL_DIR/scripts/list-skills.mjs`) and check the referenced path inside the skill's own directory. A skill in layout B has no directory — a `scripts/…` reference in a flat file has no directory to check against, so it is **not** a `missing_script`.
6. **`duplicate_name`** is detected across the whole tree (criterion 16): the same effective name appearing in both layouts.
7. **`scan_skills` emits `loom_core::SkillEntry`** — no new struct (C3). `path` points at the **file** (F13). `description` is the **parsed** description (empty string when absent — C11/F1). `version` is the frontmatter `version:` if present, else a synthesized default (C10/F12). `content_hash` is the SHA-256 of the file's bytes (the registry computes it too when empty — `registry.rs:157-163`).
8. **`description` is NOT stored in the registry row only** — the parsed description is also what A4's cue is built from. The CLI-visible description of a skill with no frontmatter is the empty string, never a placeholder that implies a description exists.

**Acceptance criteria owned by A1** (verbatim, proposal §4):

> 7. A skill with no `description:` is **refused at install**, with the file path and the missing field named in the error.
> 8. A skill with a `name:` that disagrees with its directory (layout A) is refused, with both values shown.
> 9. A flat `.md` skill with a disagreeing `name:` is **warned about, not refused** (§3.1).
> 10. `validate_skill_tree` returns **every** defect in the tree, not just the first — one call, one list.
> 11. All three YAML description styles in use today (`>`, `|`, double-quoted) parse to the same effective description. A regression test uses the real `ponytail/SKILL.md` shape (`description: >` with a folded multi-line value) and the real `ui-ux-pro-max` shape (a quoted value).
> 12. A quoted `name:` (`name: "graphflow"`) does **not** report a false `name_mismatch`.
> 13. Unknown frontmatter keys (`triggers`, `argument-hint`, `license`) are preserved and do not make a skill invalid.
> 14. A skill with an unterminated `---` fence is reported as `invalid_frontmatter`, not silently treated as "no frontmatter".
> 15. A skill that references a `scripts/…` file that does not exist in its directory is reported as `missing_script` — the live `open-design` skill is the fixture.
> 16. The two flat/dir copies of one skill are reported as `duplicate_name`, not silently double-counted.

*(A1 owns the **detection** for 7–16; A3 owns the **refusal** for 7 and 8.)*

**Tests A1 must run:**
- `cargo test -p loom-skills` — inline `#[cfg(test)] mod tests` in `crates/loom-skills/src/scan.rs`, one test per case, all using `tempfile::tempdir()`:
  - `missing_description_is_fatal` — a fixture with no `description:` → one `missing_description`, `Severity::Fatal`.
  - `name_mismatch_layout_a_is_fatal` — dir `alpha/` + `name: beta` → `name_mismatch`, Fatal, message shows **both** values.
  - `name_mismatch_layout_b_is_warn` — flat `alpha.md` + `name: beta` → `name_mismatch`, **Warn**.
  - `quoted_name_is_not_a_mismatch` — `name: "graphflow"` in `graphflow/` → **no** `name_mismatch` (shape copied inline, C14).
  - `folded_literal_and_quoted_descriptions_parse_identically` — three fixtures (`>`, `|`, `"…"`), same expected effective text; the folded fixture copies the real `ponytail` shape (multi-line folded value, C14).
  - `unknown_frontmatter_keys_are_preserved` — a fixture with `triggers`, `argument-hint`, `license` → no defect, keys preserved.
  - `unterminated_fence_is_invalid_frontmatter` — an opening `---` with no closing fence → `invalid_frontmatter`, Fatal (**not** "no frontmatter").
  - `empty_body_is_warn` — frontmatter only, no body → `empty_body`, Warn.
  - `missing_script_is_reported` — the `open-design` shape: body references `$SKILL_DIR/scripts/list-skills.mjs`, directory contains only `SKILL.md` → `missing_script`, Warn (C4).
  - `duplicate_flat_dir_pair_is_reported` — `alpha/SKILL.md` + `alpha.md` → one `duplicate_name`, Warn, and the tree reports **one** skill, not two.
  - `validate_skill_tree_returns_every_defect` — one tree with **≥3** distinct defects → the returned list has all of them, not just the first.
  - `empty_tree_returns_no_defects` — an empty skills dir → empty vec, no panic.
  - `nested_md_inside_skill_dir_is_not_a_skill` — the `ui-craft` shape (`ui-craft/SKILL.md` + `ui-craft/ui-craft.md`) → **one** skill, no `duplicate_name` for the nested copy (C7).
  - `scan_skills_emits_registry_ready_entries` — the emitted `SkillEntry` carries the parsed description, the file path, a non-empty version, and a content hash (C10, F13).
- `cargo check --workspace` and `cargo clippy --workspace` clean.
- `cargo test -p loom-skills` green **before** A1 merges (A2/A4 depend on it).

---

### A2 — Registry population (A0: the inventory becomes real)

**Purpose.** Make the filesystem the registry's input, so `loom skills list`, `loom skills show`, `loom lock --generate`, the daemon, the MCP server and the TUI all see the installed skills.

**Write scope (exclusive).**
- `crates/loom-skills/src/registry.rs` — add the disk→registry population path (call `scan_skills`, `upsert` each entry, prune rows for skills that vanished).
- `crates/loom-skills/src/lib.rs` — add the `SkillsEngine` refresh wiring (one line per the shared-file rule).
- `crates/loom-cli/src/commands/skills.rs` — rewrite the **`list`** (`:20-114`) and **`show`** (`:115-160`) handlers so both resolve through the registry. *(The `install`/`remove` handlers belong to A3 — see the split rule below.)*
- `crates/loomd/src/ipc_handler.rs` — refresh the registry before serving `skills.list` (`:68-71`) and `skills.show` (`:72-86`).
- `crates/loom-cli/src/commands/lock.rs` — **no change** (F16).

**Depends on:** **A1** (needs `scan_skills`).

**Split rule between A2 and A3 in `crates/loom-cli/src/commands/skills.rs`:** both cards edit the same file. **A2 owns `list` and `show`; A3 owns `install` and `remove`.** They may land in either order; the loser rebases. `update_lock` (`:6-16`) is A3's, because it is called only from install/remove.

**What A2 must implement:**
1. **A refresh path**: `SkillRegistry` gains a method that runs `scan_skills` over `<loom_dir>/skills` and upserts every entry, and **removes registry rows whose skill is no longer on disk** (otherwise the registry only ever grows and criterion 6 fails after a removal).
2. **The refresh runs (a) when the registry is opened** (`registry.rs:20` `SkillRegistry::new`) and **(b) on every daemon `skills.list`/`skills.show`** (F18 — the daemon's registry is constructed once at `crates/loomd/src/lib.rs:58`, so an open-time-only population is stale for the daemon's lifetime). The daemon path is the one criterion 1 exercises.
3. **`list` and `show` go through the registry in both the daemon and fallback paths** (C8, C9). The fallback must no longer be a raw directory listing (`skills.rs:47-54`) and `show` must read `SkillEntry.path` — the **file** — not the directory (F11, F13).
4. **A Fatal-defective skill still gets a registry row** (C12) — `gdrive` is one of the 11 and must be counted. Validation annotates; it never removes an entry from the inventory.
5. **No schema change** (C11). The validation state is **not** a registry column; `loom skills list` presents the defect list joined by name. *(The joining happens in A2's `list` handler: call `validate_skill_tree` and map defects by skill name.)*

**Acceptance criteria owned by A2** (verbatim, proposal §4):

> 1. `loom skills list` on a tree with 11 skills returns **11**, not `[]` — the registry is populated from the filesystem.
> 2. `loom skills show <name>` finds a skill that exists on disk, in either layout.
> 3. `loom lock --generate` writes a lock file with one entry per installed skill.
> 6. The three inventories agree: `skills.db` rows == the number of distinct skills on disk == the lock file entries.

**Tests A2 must run:**
- `cargo test -p loom-skills` (inline tests in `crates/loom-skills/src/registry.rs`, `tempfile::tempdir()`):
  - `walk_populates_registry_rows` — a temp tree with 3 skills → `registry.count() == 3`.
  - `flat_and_dir_pair_is_one_row` — `alpha/SKILL.md` + `alpha.md` → **1** row, `name == "alpha"`.
  - `fatal_skill_still_gets_a_row` — a `gdrive`-shaped skill (no frontmatter) → 1 row, `description == ""` (C12).
  - `version_is_synthesized_when_frontmatter_has_none` — a row exists and `version != ""` (C10/F12).
  - `refresh_removes_rows_for_vanished_skills` — scan, delete a skill from disk, refresh → the row is gone (needed for criterion 6 after a removal).
  - `registry_hash_matches_the_skill_file` — `content_hash` is the SHA-256 of the file bytes, and `path` is the file (F13).
- `cargo test -p loom` (inline tests in `crates/loom-cli/src/commands/skills.rs`):
  - `skills_list_returns_the_registry_contents` — daemon-down path, temp dir with 3 skills → the list has 3 entries and **no** `skills.db`/`skills.lock` noise (C8).
  - `skills_show_finds_a_directory_skill` — `show` on a layout-A skill returns the skill instead of `EISDIR` (C9).
- **Manual acceptance against a sandbox tree** (never the live `~/.loom/`):
  ```
  export LOOM_AGENTS_DIR=$(mktemp -d)
  # populate $LOOM_AGENTS_DIR/skills with 11 skills (dirs + flats)
  loom skills list --json   # → "count": 11
  loom skills show ponytail --json   # → "found": true
  loom lock --generate      # → "✓ Lock file generated … (11 skill(s))"
  sqlite3 "$LOOM_AGENTS_DIR/skills/skills.db" "select count(*) from skills;"   # → 11
  ```
  and the same three commands with the daemon running, to prove both paths agree (C8).
- `cargo check --workspace` and `cargo clippy --workspace` clean.

---

### A3 — Install / remove, both layouts, with validation

**Purpose.** Fix the two write paths: `loom skills install` accepts a directory **and** a flat file and refuses Fatal-defective skills; `loom skills remove` removes either form and leaves nothing behind.

**Write scope (exclusive).**
- `crates/loom-cli/src/commands/skills.rs` — the **`install`** handler (`:161-176`), the **`remove`** handler (`:177-186`), `update_lock` (`:6-16`), and the inline `#[cfg(test)] mod tests`.
- *(A2 owns `list`/`show` in the same file — see the split rule.)*

**Depends on:** **A1** (the validator) and **A2** (the registry, so an install is registered and the lock reflects it).

**What A3 must implement:**
1. **Install accepts both forms.** A **directory** source is copied as a tree into `<loom_dir>/skills/<dir-name>/` (the layout-A form); a **file** source is copied as `<loom_dir>/skills/<file-name>` (the layout-B form). The destination rule is the inverse of the current bug (`skills.rs:171` keeps the source file name — that is right for a file and wrong for a directory, which today never gets copied at all).
2. **Install validates before copying.** A Fatal defect **refuses the install** with the **file path** and the **missing field** named in the error (criterion 7), and for a layout-A `name_mismatch` with **both values** shown (criterion 8). A layout-B `name_mismatch` is **warned about, not refused** (criterion 9). A `missing_script` is a **Warn** on day one (C4) — it must not block an install.
3. **Install registers the skill.** After a successful copy, the registry row is upserted and `update_lock` is called — so `skills.db` and `skills.lock` both gain the skill (today `update_lock` reads an empty registry and the lock never gains anything — F10).
4. **Remove removes either form.** A **directory** skill is removed recursively (today `remove_file` fails with *"Operation not permitted (os error 1)"*); a **flat file** skill is removed. The name resolution must accept both `alpha` and `alpha.md` — today `remove` joins the raw name, so the installed file `alpha.md` is not found unless the extension is supplied (F3).
5. **Remove leaves nothing behind**: no registry row, no lock entry, no file or directory on disk.
6. **`update_lock` is fixed**: it must lock what is now in the registry (after A2's population), not an empty list.

**Acceptance criteria owned by A3** (verbatim, proposal §4):

> 4. `loom skills install <dir>` installs a `<name>/SKILL.md` directory; `loom skills install <file.md>` installs a flat file. Both succeed. *(The directory case fails today — §1.1.1.)*
> 5. `loom skills remove <name>` removes both a flat file and a directory skill, leaving nothing behind. *(The directory case fails today — §1.1.2.)*
> 7. A skill with no `description:` is **refused at install**, with the file path and the missing field named in the error.
> 8. A skill with a `name:` that disagrees with its directory (layout A) is refused, with both values shown.
> 9. A flat `.md` skill with a disagreeing `name:` is **warned about, not refused** (§3.1).

**Tests A3 must run:**
- `cargo test -p loom` (inline tests in `crates/loom-cli/src/commands/skills.rs` — the CLI package is named `loom`, C13; `tempfile::tempdir()`):
  - `install_directory_skill_round_trip` — install a `<name>/SKILL.md` dir → the dir exists at the destination, `registry.count()` +1, the lock gains the entry, and `remove` deletes it → dir gone, row gone, lock entry gone, "leaving nothing behind" (criteria 4, 5, 6).
  - `install_flat_file_round_trip` — the same for a flat `<name>.md`.
  - `install_refuses_missing_description_naming_path_and_field` — the error mentions the **path** and **description** (criterion 7).
  - `install_refuses_layout_a_name_mismatch_showing_both_values` — the error mentions the directory name **and** the frontmatter `name:` (criterion 8).
  - `install_flat_name_mismatch_warns_not_refuses` — the install **succeeds** and a warning naming both values is printed (criterion 9).
  - `install_missing_script_warns_not_refuses` — day-one severity (C4).
  - `remove_accepts_name_with_or_without_extension` — `remove alpha` removes `alpha.md`; `remove beta` removes `beta/`.
- **Manual acceptance** (temp `LOOM_AGENTS_DIR`): install a directory and a file into the sandbox tree, confirm both land in the right shape, then `loom skills list` shows both and `loom lock --generate` counts both.
- `cargo test -p loom-skills` still green (A1/A2 unaffected).
- `cargo check --workspace` and `cargo clippy --workspace` clean.

---

### A4 — Trigger generation

**Purpose.** Produce one trigger line per valid skill so the agent learns the skill exists — deterministically, under a cap, with the cue taken from the **parsed** YAML value.

**Write scope (exclusive).**
- `crates/loom-skills/src/trigger.rs` — **new**.
- `crates/loom-skills/src/lib.rs` — add `pub mod trigger;` (one line; see the shared-file rule).

**Depends on:** **A1** for integration (the cue source is the parsed description). **The module itself has no dependency on A1's internals** — it takes `&[SkillEntry]` and a cue callback — so its *code* may be written in parallel with A1; its *integration* waits for A2 to produce entries.

**Public API (canonical, proposal §3.4; analysis §12.1):**
```rust
use std::collections::BTreeMap;

pub enum Layout { Dir, Flat }

pub struct TriggerSet {
    pub lines: BTreeMap<String, String>,
    pub not_triggered: Vec<String>,
}

pub const TRIGGER_MAX_CHARS: usize = 4_000;

pub fn trigger_lines(skills: &[loom_core::SkillEntry], cue_of: impl Fn(&loom_core::SkillEntry) -> &str) -> TriggerSet;
```

**What A4 must implement:**
1. **One line per valid skill**, in **deterministic order** — `BTreeMap` keyed by the effective skill name (criterion 19). A set that reorders between runs makes diffs meaningless.
2. **The cap is enforced by dropping, and the drop is reported** — every skill whose line would push the total past `TRIGGER_MAX_CHARS` is named in `not_triggered` (criterion 20). Never silently dropped.
3. **The cue is the parsed YAML value** — a folded `description: >` yields the **parsed text**, never the literal `>` (criterion 21). The cue is truncated to a visible budget (the line is a pointer, not a copy); the truncation is **visible in the line**, not silent.
4. **Only valid skills get a line.** A skill with a Fatal defect is not triggered (it cannot work). A skill with only Warn defects still is.
5. **Consumers (proposal §3.4):** `loom skills list` prints the generated trigger set, and `loom skills install` writes it. *Where* the written set lands is a **deferred product decision** (option A `~/.loom/skills/TRIGGERS.md` or option C an env var; option B — real prompt injection — is **out of scope**, proposal §3.6). A4 ships the generator plus the `loom skills list` consumer; the `install` consumer writes to the chosen location **only if the product decision is made before A4 merges** — otherwise A4 prints the set from `list` and records the decision as open. **A4 must not introduce a prompt-injection path.**

**Acceptance criteria owned by A4** (verbatim, proposal §4):

> 19. Trigger lines are generated for every valid skill, in a deterministic order.
> 20. A skill whose trigger line would exceed `TRIGGER_MAX_CHARS` is named in `not_triggered` — never silently dropped.
> 21. A folded description (`description: >`) yields a cue that is the **parsed text**, not the literal `>`.

**Tests A4 must run:**
- `cargo test -p loom-skills` (inline tests in `crates/loom-skills/src/trigger.rs`, `tempfile::tempdir()`; `SkillEntry` values are built inline — no filesystem needed):
  - `trigger_lines_are_deterministic` — the same skill set produces byte-identical output across two calls, and the key order is sorted (criterion 19).
  - `every_valid_skill_gets_a_line` — N valid skills → N lines; a Fatal-defective skill gets none.
  - `over_cap_skill_is_reported_not_dropped` — skills whose lines exceed `TRIGGER_MAX_CHARS` appear in `not_triggered` by name, and the total stays within the cap (criterion 20).
  - `cap_boundary_is_exact` — a skill whose line lands exactly at the cap is included; one character more is excluded and reported.
  - `folded_description_yields_parsed_cue` — an entry whose description is the folded text yields that text in the line — **not** `>` (criterion 21).
  - `cue_comes_from_the_cue_of_callback` — the callback's value is what appears in the line.
- `cargo test -p loom` — the `loom skills list` consumer prints the trigger set (A4's consumer lives in the CLI's `list` handler; coordinate with A2, which owns that handler — **A4 supplies the call, A2 owns the file**).
- `cargo check --workspace` and `cargo clippy --workspace` clean.

---

### A5 — Sync gate (both paths) + `loom doctor` row

**Purpose.** A defective skill must not reach a tool's native skills directory, and the validation state must be visible in `loom doctor`.

**Write scope (exclusive).**
- `crates/loom-adapters/src/lib.rs` — `sync_skills_to_native` (`:227-273`) and the note plumbing in `sync_to_native` (`:46`, `:72-83`).
- `crates/loom-cli/src/commands/exec.rs` — `inject_shared_skills` (`:518-559`) and its call site (`:422`).
- `crates/loom-cli/src/commands/doctor.rs` — the new check row (`:50` `run`, row shape `:29-44`).
- `crates/loom-cli/src/commands/sync.rs` — **no change expected** (`:33-38` already collects `report.notes`).

**Depends on:** **A1** (the shared validator). **Independent of A2 and A3** — it can proceed in parallel with them once A1 merges.

**What A5 must implement:**
1. **One shared rule, two call sites.** Both sync paths call the **same** predicate from `loom_skills::scan` — `validate_skill_file(path, dir_name)` filtered to `Severity::Fatal` — so the two paths **cannot disagree** (criterion 18). `loom-adapters` already depends on `loom-skills` (`crates/loom-adapters/Cargo.toml:11`) and `loom-cli` depends on it too (`crates/loom-cli/Cargo.toml:21`) — no new dependency.
   - `loom-adapters` path (layout A): `validate_skill_file(&dir.join("SKILL.md"), Some(&dir_name))`.
   - `loom-cli` path (layout B): `validate_skill_file(&file, None)`.
   - **No duplicated rule, no duplicated parsing, no second frontmatter parser** (F6 — the tree currently has none).
2. **A Fatal-defective skill is skipped, not hard-failed.** A hard error would abort the whole sync for one bad skill. The skip is **named in `SyncReport.notes`** (criterion 17). Because `sync_skills_to_native` returns `Result<usize>` and cannot carry notes, its return type must widen to carry the skip notes (e.g. `Result<(usize, Vec<String>)>`), and `sync_to_native` (`:72-83`) pushes each note into `report.notes`. `SyncReport.notes` (`:313`) already reaches the user through `loom sync` (`crates/loom-cli/src/commands/sync.rs:37-38`) and `loom thread` (`crates/loom-cli/src/commands/thread.rs:35`).
3. **`inject_shared_skills` applies the identical rule** (criterion 18). Note the asymmetry that remains **by design**: the adapters path covers layout A only and the exec path covers layout B only (F4, F5). This slice makes the **rule** shared; it does **not** unify which layouts each path covers — unifying coverage is not in any criterion and is out of scope.
4. **`loom doctor` gets one row** (criterion 22): name `"Skill validation"`, status `Pass` / `Warn` / `Fail` from the defect counts, detail naming the skill and the defect, plus a `with_explanation` note (F15). `doctor.rs` needs **no new plumbing** — it already builds a `Vec<CheckRow>` and renders a `comfy_table::Table`.
   - The row must name the live `gdrive` defect by name (proposal §8 definition of done).

**Acceptance criteria owned by A5** (verbatim, proposal §4):

> 17. `sync_skills_to_native` does not symlink a Fatal-defective skill into a native tool directory, and the skip is named in the `SyncReport` notes.
> 18. `inject_shared_skills` (the layout-B path) applies the same rule as `sync_skills_to_native` (the layout-A path). The two paths cannot disagree.
> 22. `loom doctor` reports the skill validation state as a row.

**Tests A5 must run:**
- `cargo test -p loom-adapters` (the crate has **no** `#[cfg(test)] mod tests` today — A5 adds one inline in `crates/loom-adapters/src/lib.rs`; `tempfile::tempdir()`):
  - `sync_skills_to_native_skips_a_fatal_skill_and_names_it` — a temp skills dir with one valid and one Fatal-defective skill → only the valid one is symlinked into the native dir, and the returned notes **name** the skipped skill and its defect code (criterion 17).
  - `sync_skills_to_native_syncs_a_valid_skill` — the happy path still symlinks.
- `cargo test -p loom` (inline tests):
  - `inject_shared_skills_skips_a_fatal_skill` — the layout-B path skips the same kind of defect (criterion 18).
  - `both_sync_paths_agree_on_the_same_file` — one fixture file is judged identically by the two paths (the shared predicate is literally the same function; the test pins that).
  - `doctor_reports_a_skill_validation_row` — a temp loom dir with a Fatal skill → the doctor output contains a row whose detail names the skill and the defect (criterion 22); `doctor` on a clean tree reports `Pass` (existing doctor tests at `doctor.rs:527+` must still pass).
- `cargo check --workspace` and `cargo clippy --workspace` clean.

---

## 4. Sequencing

### Dependency graph
```
        A1 scan+parser ──────────┬──────────────┬────────────────┐
        (serde_yaml, scan.rs,    │              │                │
         delete neutral_renderer)│              │                │
                                │              │                │
                    ┌───────────┴──┐      ┌────┴─────┐    ┌─────┴──────┐
                    │              │      │          │    │            │
              A2 registry     A4 trigger     │          │    │            │
              (skills.db     (trigger.rs)   │          │    │            │
               populated,                     │          │    │            │
               list/show,                     │          │    │            │
               daemon refresh)                │          │    │            │
                    │              │          │          │    │            │
                    └──────┬───────┘          │          │    │            │
                           │                  │          │    │            │
                      A3 install/remove ──────┘          │    │            │
                      (both layouts, refuse Fatal)      │    │            │
                                                        │    │            │
                                                  A5 sync-gate + doctor ─┘
                                                  (shared predicate, doctor row)
```

### What can run in parallel, and what must wait
| Cards | Parallel? | Rule |
|---|---|---|
| **A1** | — | Root card. Must land first: every other card needs `scan_skills` / `validate_skill_file`. |
| **A4** | **Yes — with A1** | `trigger.rs` takes `&[SkillEntry]` and a cue callback, so its *code* is independent of A1's internals. Only its *integration* (the `loom skills list` consumer) waits for A2. **A4's `lib.rs` line waits for A1's** (shared-file rule). |
| **A2** | **Yes — with A4** | Needs A1 landed. Independent of A3 and A5. |
| **A5** | **Yes — with A2 and A3** | Needs A1 landed (the shared predicate). Independent of the registry work — it validates files directly. |
| **A3** | **No — waits for A2** | Needs A1 (the validator) and A2 (the registry, so an install is registered and the lock is meaningful). |

**Critical path:** `A1 → A2 → A3` (3 cards). **A4 and A5 are off the critical path** and can be built in parallel with it once A1 lands.

**Merge order (to keep `lib.rs` conflict-free):** A1 → (A4 ∥ A2) → (A5 ∥ A3).

**The one file two cards share beyond `lib.rs`:** `crates/loom-cli/src/commands/skills.rs` — A2 owns `list`/`show`, A3 owns `install`/`remove`/`update_lock`. They may land in either order; the second rebases.

**Definition-of-done gate for the whole slice** (proposal §8, checked at the end, after A5):
- `serde_yaml` is in the root `[workspace.dependencies]` and in `crates/loom-skills/Cargo.toml`; `crates/loom-skills/src/neutral_renderer.rs` is deleted; `scan.rs` and `trigger.rs` exist.
- The walk handles both layouts and populates `skills.db`; the three inventories agree.
- `loom skills list` returns the installed skills; `loom skills show <name>` finds them; `loom lock --generate` writes one entry per skill.
- `loom skills install` accepts a directory **and** a flat file; `loom skills remove` removes both forms.
- `validate_skill_tree` is implemented for both layouts with all seven defect codes.
- `trigger_lines` implements the cap, the `not_triggered` report, and a cue from the **parsed** YAML value.
- The install path refuses Fatal defects; both sync paths skip Fatal defects and report them.
- `loom doctor` shows a skill-validation row; `loom skills list` shows per-skill validation state and the trigger set.
- `cargo test -p loom-skills` green, `cargo test -p loom` green, `cargo test -p loom-adapters` green, `cargo check --workspace` green, `cargo clippy --workspace` green.
- `loom doctor` run against a **live** `~/.loom/` names the `gdrive` defect.

---

## 5. Out of scope — prohibitions

Everything below is **cut** and must **not** re-enter through any card. If a card "needs" one of these, it stops and raises a decision instead of building it.

### 5.1 The analysis §7.1 cut list, restated as prohibitions
Source: `docs/LOOM_HARNESS2_ANALYSIS.md:490-500`.

1. **No `SurfaceSpec` as serde-deserialisable rules-as-data** (analysis §6.2). Cut: a schema with no author today; upstream's validator is plain functions. *No `SurfaceSpec` type, no rules-as-data file, no deserialisable rule schema in any card.*
2. **No `~/.loom/surfaces/<tool>/` directory, no `manifest.toml`, no `sync_surface_to_native`.** Cut: a second source of truth beside `~/.loom/tools/*.toml`, for a surface that does not exist. *No new directory under `~/.loom/`, no new manifest format, no new sync function.*
3. **No `loom surface show/validate/report` CLI group.** Cut: 36 top-level commands already; `loom doctor` + `loom skills` cover the need. *No new CLI group, no new top-level command.*
4. **No `~/.loom/experience/`, no `INDEX.md`, no knowledge bridge.** Cut: one `lesson` entity in `loom-hindsight` + `loom memory retain --type lesson` is enough. *No experience store, no lesson index.*
5. **No full-tree retention, no grader-verdict assertion.** Cut: upstream retains the tree because the tree is the artefact; Loom's artefact is the lesson. *No tree snapshotting of the whole loom dir, no verdict model.*
6. **No `loom-harness` crate.** Cut: `ROADMAP.md` pillar 1 — no new crate without proof of duplication. *No new workspace member. `serde_yaml` into the existing `loom-skills` crate is the only dependency addition.*
7. **The original Phase 0–4 plan (38–59 days) is not revived.** Phase A (this slice) + Phase B (the cheap experiment, 2–3 days, $10–20, a separate card) replace it. *No probe loop, no contrast harness, no selection machinery.*

### 5.2 The proposal §3.6 prohibitions, restated
Source: `docs/plan/LOOM_SKILL_VALIDATION_PROPOSAL.md:321-331`.

8. **No always-on prompt injection.** Option B of §3.4 is deferred. *No card writes into `AGENTS.md`, `systemprompt.md`, or any per-tool always-on file; no card injects trigger text into an agent's prompt.*
9. **No experience library, no probe loop, no selection.** (Analysis Phases B–E.)
10. **No change to the `skills.db` schema.** The `description` column already exists (`registry.rs:34`); A0 **populates** the existing schema, it does not extend it. **No column, no migration, no backfill** — including no "validation state" column (C11).
11. **No migration or deletion of the 6 duplicate flat `.md` skills.** The validator **names** the collision (`duplicate_name`); resolving it is a user decision. *No card may move, merge, rename or delete `~/.loom/skills/ponytail*.md`.*
12. **No repair of `neutral_renderer.rs` — it is deleted**, not revived (F6). *No card may re-declare the module, restore the file, or copy its `NeutralRenderer`.*
13. **No change to `snapshot.db`.** It is a discovery snapshot with a different job; it is not the registry. *No card writes to `~/.loom/.cache/snapshot.db`; `crates/loom-discovery/src/initializer.rs` is not in any card's write scope.*

### 5.3 Boundaries this brief adds
14. **No new CLI command, group, or flag** beyond behaviour changes inside existing commands. *No `loom skill validate`, no `loom skill-*`, no `--strict` flag, no new subcommand.*
15. **No dependency other than `serde_yaml`.** *No `serde_yml`, no `serde_norway`, no `yaml-rust` (the cached `yaml-rust 0.4.5` is not serde-based and would mean hand-rolling scalar handling).*
16. **No new "what a skill is" type.** The walk emits `loom_core::SkillEntry` (C3). *No `SkillMeta`, no `SkillFrontmatter` revival, no new skill struct.*
17. **No third skill layout.** Layout A (`<dir>/SKILL.md`) and layout B (flat `<name>.md`) are the only two (C7). *A nested `.md` inside a layout-A directory is not a layout.*
18. **No migration of the live `~/.loom/` tree.** The 6 duplicates stay (11); the `gdrive` skill stays on disk and stays visible in `loom doctor` (F22). *No card may write to the real `~/.loom/` during development or testing.*
19. **No prompt-injection or trigger-file landing decision.** Where the trigger text lands is a deferred product decision (option A vs C); **option B is prohibited** (8). A4 ships the generator plus the `loom skills list` consumer; if the landing decision is not made before A4 merges, A4 prints the set and records the decision as open.

---

## 6. Coverage matrix — every §4 criterion has an owner and a test

All **23** criteria from proposal §4, each with its owning card and the test that proves it. **No criterion is dropped.**

| # | Criterion (short form) | Owning card | Test |
|---|---|---|---|
| 1 | `loom skills list` returns 11, not `[]` | **A2** | `skills_list_returns_the_registry_contents` + manual `LOOM_AGENTS_DIR` run (`"count": 11`) |
| 2 | `loom skills show <name>` finds a skill in either layout | **A2** | `skills_show_finds_a_directory_skill` + manual `loom skills show ponytail` → `found: true` |
| 3 | `loom lock --generate` writes one entry per installed skill | **A2** | manual run → `(11 skill(s))`; `lock.rs` unchanged (F16) |
| 4 | `install <dir>` and `install <file.md>` both succeed | **A3** | `install_directory_skill_round_trip`, `install_flat_file_round_trip` |
| 5 | `remove <name>` removes both forms, leaves nothing behind | **A3** | `install_directory_skill_round_trip` / `install_flat_file_round_trip` (full round-trip) |
| 6 | the three inventories agree | **A2** (+A3) | `walk_populates_registry_rows`, `flat_and_dir_pair_is_one_row`, `refresh_removes_rows_for_vanished_skills` + manual sqlite count |
| 7 | no `description:` → refused at install, path + field named | **A1** (detect) + **A3** (refuse) | A1: `missing_description_is_fatal`; A3: `install_refuses_missing_description_naming_path_and_field` |
| 8 | layout-A `name:` mismatch refused, both values shown | **A1** (detect) + **A3** (refuse) | A1: `name_mismatch_layout_a_is_fatal`; A3: `install_refuses_layout_a_name_mismatch_showing_both_values` |
| 9 | flat `.md` mismatch warned, not refused | **A1** (severity) + **A3** (behaviour) | A1: `name_mismatch_layout_b_is_warn`; A3: `install_flat_name_mismatch_warns_not_refuses` |
| 10 | `validate_skill_tree` returns **every** defect | **A1** | `validate_skill_tree_returns_every_defect` |
| 11 | all three YAML description styles parse alike | **A1** | `folded_literal_and_quoted_descriptions_parse_identically` |
| 12 | quoted `name:` is not a false mismatch | **A1** | `quoted_name_is_not_a_mismatch` |
| 13 | unknown frontmatter keys preserved, not invalid | **A1** | `unknown_frontmatter_keys_are_preserved` |
| 14 | unterminated `---` → `invalid_frontmatter` | **A1** | `unterminated_fence_is_invalid_frontmatter` |
| 15 | missing referenced script → `missing_script` | **A1** | `missing_script_is_reported` (the `open-design` shape) |
| 16 | flat/dir pair → `duplicate_name`, not double-counted | **A1** (+A2) | A1: `duplicate_flat_dir_pair_is_reported`; A2: `flat_and_dir_pair_is_one_row` |
| 17 | `sync_skills_to_native` skips a Fatal skill, skip named in `SyncReport.notes` | **A5** | `sync_skills_to_native_skips_a_fatal_skill_and_names_it` |
| 18 | `inject_shared_skills` applies the same rule; the two cannot disagree | **A5** | `inject_shared_skills_skips_a_fatal_skill`, `both_sync_paths_agree_on_the_same_file` |
| 19 | trigger lines for every valid skill, deterministic order | **A4** | `trigger_lines_are_deterministic`, `every_valid_skill_gets_a_line` |
| 20 | over-cap skill named in `not_triggered`, never silently dropped | **A4** | `over_cap_skill_is_reported_not_dropped`, `cap_boundary_is_exact` |
| 21 | folded `description: >` yields the parsed text as the cue | **A4** (+**A1**) | A4: `folded_description_yields_parsed_cue`; A1: `folded_literal_and_quoted_descriptions_parse_identically` |
| 22 | `loom doctor` reports the skill validation state as a row | **A5** | `doctor_reports_a_skill_validation_row` |
| 23 | `cargo test -p loom-skills` covers the ten listed cases | **A1** + **A4** (+**A3**) | A1: `missing_description_is_fatal`, `name_mismatch_layout_a_is_fatal`, `name_mismatch_layout_b_is_warn`, `quoted_name_is_not_a_mismatch`, `folded_literal_and_quoted_descriptions_parse_identically`, `unterminated_fence_is_invalid_frontmatter`, `empty_tree_returns_no_defects`, `missing_script_is_reported`, `duplicate_flat_dir_pair_is_reported`; A4: `cap_boundary_is_exact`; A3: the **directory install + remove round-trip** — which lives in `cargo test -p loom` (**package name is `loom`**, C13) |

**Criterion 23 caveat (C13).** As written in the proposal, criterion 23 asks `cargo test -p loom-skills` to cover the directory install + remove round-trip. That is **not possible**: the install/remove code is in the CLI crate, whose package name is `loom` (`crates/loom-cli/Cargo.toml:2`), and `cargo run -p loom-cli` does not resolve. The criterion is therefore met by the **same coverage split across two commands** — nine cases in `cargo test -p loom-skills` (A1/A4), the cap-boundary case in `cargo test -p loom-skills` (A4), and the install/remove round-trip in `cargo test -p loom` (A3). **Nothing is dropped; only the package attribution is corrected.**

---

## 7. Open decisions (not blockers for A1–A3)

| # | Decision | Owner | Blocks |
|---|---|---|---|
| D1 | Where the generated trigger text lands — option A (`~/.loom/skills/TRIGGERS.md`) or option C (env var). Option B (prompt injection) is prohibited. | product | A4's *install* consumer only; A4 ships the generator + `loom skills list` consumer either way |
| D2 | When to widen `missing_script` from Warn to Fatal (C4) — it would refuse the live `open-design` skill at install. | product | nothing in this slice; A1/A3 implement the C4 table |
| D3 | Whether to widen `name_mismatch` in layout A beyond Fatal (it is already Fatal on day one per C4, so the question is only whether to keep it that way). | product | nothing |
| D4 | The duplicate flat/dir pairs: warn forever, or migrate the 6 flat files to layout A. §3.6 forbids migration **in this slice** (prohibition 11). | user | nothing in this slice |

---

## 8. Verify-before-you-start checklist (for any card)

Before writing code, a card must be able to answer "yes" to all of these from the **live tree at the branch tip**:

1. `crates/loom-skills/src/registry.rs:34` still reads `description TEXT NOT NULL` (F1).
2. `crates/loom-skills/src/registry.rs:38` still reads `version TEXT NOT NULL` (F12).
3. `crates/loom-core/src/types.rs:282` is still `pub struct SkillEntry` (F1).
4. `crates/loom-skills/src/lib.rs` does **not** declare `neutral_renderer` and **does** declare `scan` (F6).
5. `grep -rn "NeutralRenderer" crates/ packages/` returns only hits inside `crates/loom-skills/src/neutral_renderer.rs` — or the file is gone (F6).
6. `grep -n serde_yaml Cargo.toml` matches in the root manifest and in `crates/loom-skills/Cargo.toml` (F6, C5).
7. `grep -rn "\.upsert(" crates/ packages/` shows **no** production caller of `loom_skills::registry::SkillRegistry::upsert` outside the registry's own tests (F10) — this is the gap A2 closes.
8. `sqlite3 ~/.loom/skills/skills.db "select count(*) from skills;"` still returns **0** on a fresh tree (F7) — the "before" picture for criterion 6.
9. `find ~/.loom/skills -mindepth 2 -name "*.md"` still lists `ui-craft/ui-craft.md` (C7) — the trap the walker must not fall into.
10. `~/.loom/skills/open-design/` still contains only `SKILL.md` while its `SKILL.md:156-160` references three `scripts/*.mjs` files (F22) — the fixture for criterion 15.
