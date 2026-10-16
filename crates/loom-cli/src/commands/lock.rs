use anyhow::{Context, Result};
use loom_skills::lockfile::{self, SkillsLock};

/// `loom lock --generate` — regenerate lock file from current state.
/// `loom lock --verify` — verify installed skills match lock file.
/// `loom lock --verify --diff` — show detailed diff of violations.
pub fn run(loom_dir: &str, generate: bool, verify: bool, diff: bool) -> Result<()> {
    let lock_path = lockfile::default_lock_path(loom_dir);

    if generate {
        let engine =
            loom_skills::SkillsEngine::new(loom_dir).context("Failed to open skill registry")?;
        let registry_skills = engine.registry().list();

        let mut lock = SkillsLock::new();
        for skill in &registry_skills {
            lock.lock_skill(skill);
        }
        lock.save(&lock_path)?;
        println!(
            "✓ Lock file generated at {} ({} skill(s))",
            lock_path.display(),
            registry_skills.len()
        );
        Ok(())
    } else if verify {
        let lock = SkillsLock::load(&lock_path)?
            .ok_or_else(|| anyhow::anyhow!("No lock file found at {}", lock_path.display()))?;

        let engine =
            loom_skills::SkillsEngine::new(loom_dir).context("Failed to open skill registry")?;
        let registry_skills = engine.registry().list();

        let violations = lock.verify_against(&registry_skills);
        if violations.is_empty() {
            println!(
                "✓ Lock file is clean — all {} skill(s) match",
                registry_skills.len()
            );
            Ok(())
        } else {
            for v in &violations {
                if diff {
                    match v.kind {
                        lockfile::LockViolationKind::Missing => {
                            println!("  -{}.toml  # in lock but removed from registry", v.name);
                        }
                        lockfile::LockViolationKind::VersionMismatch => {
                            println!(
                                "  ~{}.toml  # version {} → {} (expected → actual)",
                                v.name, v.expected, v.actual
                            );
                        }
                        lockfile::LockViolationKind::ContentChanged => {
                            println!(
                                "  ~{}.toml  # content hash changed ({} → {})",
                                v.name,
                                &v.expected[..8],
                                &v.actual[..8]
                            );
                        }
                        lockfile::LockViolationKind::Unlocked => {
                            println!("  +{}.toml  # not in lock file", v.name);
                        }
                    }
                } else {
                    match v.kind {
                        lockfile::LockViolationKind::Missing => {
                            println!("  ✗ {}: in lock but removed from registry", v.name);
                        }
                        lockfile::LockViolationKind::VersionMismatch => {
                            println!(
                                "  ✗ {}: version mismatch (expected {}, actual {})",
                                v.name, v.expected, v.actual
                            );
                        }
                        lockfile::LockViolationKind::ContentChanged => {
                            println!("  ✗ {}: content hash changed", v.name);
                        }
                        lockfile::LockViolationKind::Unlocked => {
                            println!("  ✗ {}: not in lock file", v.name);
                        }
                    }
                }
            }
            anyhow::bail!("Lock file has {} violation(s)", violations.len());
        }
    } else {
        anyhow::bail!("Use `loom lock --generate` or `loom lock --verify`");
    }
}
