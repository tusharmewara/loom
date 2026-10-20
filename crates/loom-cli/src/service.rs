//! Supervision of the loomd service unit (launchd on macOS, systemd on Linux).
//!
//! `loom daemon install-service` writes a unit that owns the daemon's
//! lifecycle. Once such a unit is installed, `loom daemon start` must not
//! spawn a *second*, competing daemon: the supervisor is the one process that
//! should be running loomd, and the CLI has to talk to it.
//!
//! This module is the single place that knows how to ask the supervisor
//! "is loomd running under you?" and "start/stop it".

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
const SERVICE_LABEL: &str = "com.loom.loomd";
#[cfg(target_os = "linux")]
const SERVICE_UNIT: &str = "loomd.service";

/// The supervisor that may own loomd on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervisor {
    /// A launchd user agent (`~/Library/LaunchAgents/com.loom.loomd.plist`).
    #[cfg(target_os = "macos")]
    Launchd,
    /// A systemd user unit (`~/.config/systemd/user/loomd.service`).
    #[cfg(target_os = "linux")]
    Systemd,
    /// No supervisor on this platform.
    #[allow(dead_code)]
    None,
}

impl Supervisor {
    /// Name of the unit, for messages.
    pub fn unit_name(&self) -> &'static str {
        match self {
            #[cfg(target_os = "macos")]
            Supervisor::Launchd => SERVICE_LABEL,
            #[cfg(target_os = "linux")]
            Supervisor::Systemd => SERVICE_UNIT,
            Supervisor::None => "none",
        }
    }
}

/// Who owns the loomd daemon for a given ecosystem directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ownership {
    /// A service unit owns loomd and serves exactly this directory.
    Unit { supervisor: Supervisor },
    /// A service unit is installed, but it serves a *different* ecosystem
    /// directory. Starting it would put the daemon in the wrong directory.
    ForeignUnit {
        supervisor: Supervisor,
        dir: PathBuf,
    },
    /// No service unit is installed: the daemon is started directly.
    None,
}

impl Ownership {
    /// Whether a service unit owns loomd for this directory.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_supervised(&self) -> bool {
        matches!(self, Ownership::Unit { .. })
    }
}

/// Path of the launchd plist.
#[cfg(target_os = "macos")]
fn unit_path(home: &Path) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist"))
}

/// Path of the systemd user unit.
#[cfg(target_os = "linux")]
fn unit_path(home: &Path) -> PathBuf {
    home.join(".config")
        .join("systemd")
        .join("user")
        .join(SERVICE_UNIT)
}

/// Who owns loomd for `loom_dir`.
///
/// A unit only counts when its file exists — the unit file is what makes the
/// supervisor the owner, and a half-installed unit (file written, never
/// loaded) must not divert the CLI into a supervisor that is not actually
/// running the daemon.
pub fn ownership(loom_dir: &Path) -> Ownership {
    let Some(home) = home_dir() else {
        return Ownership::None;
    };
    ownership_in(&home, loom_dir)
}

/// [`ownership`] with the home directory given explicitly, so the lookup is
/// testable without mutating the process environment.
fn ownership_in(home: &Path, loom_dir: &Path) -> Ownership {
    let path = unit_path(home);
    if !path.exists() {
        return Ownership::None;
    }

    let supervisor = supervisor();
    match unit_loom_dir(&path) {
        // An unset LOOM_AGENTS_DIR means the unit uses the daemon's default
        // (`~/.loom`), which only matches a `~/.loom` ecosystem dir.
        Some(dir) if same_dir(&dir, loom_dir) => Ownership::Unit { supervisor },
        Some(dir) => Ownership::ForeignUnit { supervisor, dir },
        None => {
            let default = PathBuf::from(crate::loom_dir());
            if same_dir(&default, loom_dir) {
                Ownership::Unit { supervisor }
            } else {
                Ownership::ForeignUnit {
                    supervisor,
                    dir: default,
                }
            }
        }
    }
}

/// Compare two ecosystem directories, tolerating a trailing separator.
fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    // Only canonicalize when both sides exist: two missing paths canonicalize
    // to `None`, which would make unrelated directories compare equal.
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The platform's supervisor.
fn supervisor() -> Supervisor {
    #[cfg(target_os = "macos")]
    {
        Supervisor::Launchd
    }
    #[cfg(target_os = "linux")]
    {
        Supervisor::Systemd
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Supervisor::None
    }
}

/// Read the `LOOM_AGENTS_DIR` the unit serves, if it sets one.
fn unit_loom_dir(unit_path: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(unit_path).ok()?;

    #[cfg(target_os = "macos")]
    {
        // plist: <key>LOOM_AGENTS_DIR</key> ... <string>DIR</string>
        let key = text.find("<key>LOOM_AGENTS_DIR</key>")?;
        let rest = &text[key..];
        let open = rest.find("<string>")? + "<string>".len();
        let close = rest[open..].find("</string>")? + open;
        let dir = rest[open..close].trim();
        if dir.is_empty() {
            None
        } else {
            Some(PathBuf::from(dir))
        }
    }

    #[cfg(target_os = "linux")]
    {
        // unit: Environment=LOOM_AGENTS_DIR=DIR
        text.lines()
            .filter_map(|l| l.trim().strip_prefix("Environment=LOOM_AGENTS_DIR="))
            .map(|v| v.trim().to_string())
            .find(|v| !v.is_empty())
            .map(PathBuf::from)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = text;
        None
    }
}

/// Start the supervised daemon.
pub fn start(sup: Supervisor) -> anyhow::Result<()> {
    match sup {
        #[cfg(target_os = "macos")]
        Supervisor::Launchd => {
            let target = launchd_target()?;
            // A job that was never bootstrapped (or was booted out) cannot be
            // kicked into existence — load it first.
            if !launchd_loaded(&target) {
                bootstrap()?;
            }
            let status = Command::new("launchctl")
                .arg("kickstart")
                .arg(&target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                anyhow::bail!("launchctl kickstart failed for {SERVICE_LABEL}");
            }
            Ok(())
        }
        #[cfg(target_os = "linux")]
        Supervisor::Systemd => {
            // The unit file may have been edited since the last reload.
            let _ = Command::new("systemctl")
                .arg("--user")
                .arg("daemon-reload")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let status = Command::new("systemctl")
                .arg("--user")
                .arg("start")
                .arg(SERVICE_UNIT)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                anyhow::bail!("systemctl --user start {SERVICE_UNIT} failed");
            }
            Ok(())
        }
        Supervisor::None => anyhow::bail!("no service unit is installed"),
    }
}

/// Stop the supervised daemon.
///
/// The daemon handles SIGTERM and exits cleanly (exit 0), and both units are
/// configured to restart only on a *crash* (`KeepAlive{SuccessfulExit:false}`
/// / `Restart=on-failure`), so a clean stop is not resurrected. The unit
/// itself stays installed and enabled.
pub fn stop(sup: Supervisor) -> anyhow::Result<()> {
    match sup {
        #[cfg(target_os = "macos")]
        Supervisor::Launchd => {
            let target = launchd_target()?;
            let status = Command::new("launchctl")
                .arg("kill")
                .arg("SIGTERM")
                .arg(&target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                anyhow::bail!("launchctl kill SIGTERM failed for {SERVICE_LABEL}");
            }
            Ok(())
        }
        #[cfg(target_os = "linux")]
        Supervisor::Systemd => {
            let status = Command::new("systemctl")
                .arg("--user")
                .arg("stop")
                .arg(SERVICE_UNIT)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !status.success() {
                anyhow::bail!("systemctl --user stop {SERVICE_UNIT} failed");
            }
            Ok(())
        }
        Supervisor::None => anyhow::bail!("no service unit is installed"),
    }
}

/// Whether the supervisor currently has a live job for loomd.
///
/// This is the supervisor's own view, which is not the same as "a daemon is
/// serving the socket": a job can be loaded and idle (a clean stop) or still
/// be in its crash-restart delay.
pub fn is_running(sup: Supervisor) -> bool {
    match sup {
        #[cfg(target_os = "macos")]
        Supervisor::Launchd => {
            let Ok(target) = launchd_target() else {
                return false;
            };
            Command::new("launchctl")
                .arg("print")
                .arg(&target)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        #[cfg(target_os = "linux")]
        Supervisor::Systemd => Command::new("systemctl")
            .arg("--user")
            .arg("is-active")
            .arg(SERVICE_UNIT)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        Supervisor::None => false,
    }
}

/// The `launchctl` target for the user agent (`gui/<uid>/<label>`).
#[cfg(target_os = "macos")]
pub fn launchd_target() -> anyhow::Result<String> {
    Ok(format!("gui/{}/{SERVICE_LABEL}", detect_uid()?))
}

/// Whether the launchd job is loaded in the user domain.
#[cfg(target_os = "macos")]
fn launchd_loaded(target: &str) -> bool {
    Command::new("launchctl")
        .arg("print")
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Load the launchd plist into the user domain.
#[cfg(target_os = "macos")]
pub fn bootstrap() -> anyhow::Result<()> {
    let home = home_dir().ok_or_else(|| anyhow::anyhow!("HOME not set"))?;
    let path = unit_path(&home);
    let domain = format!("gui/{}", detect_uid()?);
    let status = Command::new("launchctl")
        .arg("bootstrap")
        .arg(&domain)
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!(
            "launchctl bootstrap failed for {SERVICE_LABEL}. Try: launchctl bootstrap {} {}",
            domain,
            path.display()
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn detect_uid() -> anyhow::Result<String> {
    let out = Command::new("id").arg("-u").output()?;
    if !out.status.success() {
        anyhow::bail!("failed to detect user id via `id -u`");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_name_is_descriptive() {
        assert_eq!(Supervisor::None.unit_name(), "none");
        #[cfg(target_os = "macos")]
        assert_eq!(Supervisor::Launchd.unit_name(), "com.loom.loomd");
        #[cfg(target_os = "linux")]
        assert_eq!(Supervisor::Systemd.unit_name(), "loomd.service");
    }

    #[test]
    fn ownership_without_a_unit_is_none() {
        let home = tempfile::tempdir().expect("temp dir");
        let own = ownership_in(home.path(), Path::new("/tmp/whatever"));
        assert_eq!(own, Ownership::None);
        assert!(!own.is_supervised());
    }

    /// Write a unit file that serves `dir` under a fake home.
    fn write_unit(home: &Path, dir: &Path) {
        let unit = unit_path(home);
        std::fs::create_dir_all(unit.parent().expect("unit parent")).expect("create unit dir");
        let body = if cfg!(target_os = "macos") {
            format!(
                "  <key>EnvironmentVariables</key>\n  <dict>\n    <key>LOOM_AGENTS_DIR</key>\n    <string>{}</string>\n  </dict>\n",
                dir.display()
            )
        } else {
            format!("[Service]\nEnvironment=LOOM_AGENTS_DIR={}\n", dir.display())
        };
        std::fs::write(&unit, body).expect("write unit file");
    }

    #[test]
    fn a_unit_for_another_directory_is_a_foreign_unit() {
        let home = tempfile::tempdir().expect("temp dir");
        write_unit(home.path(), Path::new("/tmp/other"));

        let own = ownership_in(home.path(), Path::new("/tmp/mine"));
        assert!(!own.is_supervised());
        match own {
            Ownership::ForeignUnit { dir, .. } => assert_eq!(dir, PathBuf::from("/tmp/other")),
            other => panic!("expected a foreign unit, got {other:?}"),
        }
    }

    #[test]
    fn a_unit_for_the_same_directory_owns_the_daemon() {
        let home = tempfile::tempdir().expect("temp dir");
        write_unit(home.path(), Path::new("/tmp/mine"));

        let own = ownership_in(home.path(), Path::new("/tmp/mine"));
        assert!(own.is_supervised(), "the unit owns this directory: {own:?}");
    }

    #[test]
    fn a_unit_without_an_explicit_dir_serves_the_default_directory() {
        let home = tempfile::tempdir().expect("temp dir");
        let unit = unit_path(home.path());
        std::fs::create_dir_all(unit.parent().expect("unit parent")).expect("create unit dir");
        std::fs::write(&unit, "[Service]\nExecStart=/bin/true\n").expect("write unit file");

        // The unit falls back to the daemon's default dir, so it owns that
        // directory and every other one is foreign to it.
        let own = ownership_in(home.path(), Path::new(&crate::loom_dir()));
        assert!(
            own.is_supervised(),
            "the unit owns the default dir: {own:?}"
        );

        let own = ownership_in(home.path(), Path::new("/tmp/somewhere-else"));
        assert!(matches!(own, Ownership::ForeignUnit { .. }), "{own:?}");
    }
}
