//! SFTP file sync operations using `rsync` and `scp`.

use crate::connection::SshConnection;
use std::process::Command;

/// Push a local path to a remote destination via rsync or scp.
pub fn push(local: &str, remote: &str, conn: &SshConnection, use_rsync: bool) -> anyhow::Result<()> {
    if use_rsync {
        let mut cmd = Command::new("rsync");
        cmd.arg("-avz")
            .arg("-e")
            .arg(ssh_base_args(conn))
            .arg(local)
            .arg(remote);
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("rsync failed: {}", stderr.trim());
        }
        Ok(())
    } else {
        let mut cmd = Command::new("scp");
        cmd.arg("-P").arg(conn.port.to_string());
        if let crate::connection::SshAuth::KeyFile(ref path) = conn.auth {
            cmd.arg("-i").arg(path);
        }
        cmd.arg(local).arg(remote);
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("scp failed: {}", stderr.trim());
        }
        Ok(())
    }
}

/// Pull a remote path to a local destination via rsync or scp.
pub fn pull(remote: &str, local: &str, conn: &SshConnection, use_rsync: bool) -> anyhow::Result<()> {
    if use_rsync {
        let mut cmd = Command::new("rsync");
        cmd.arg("-avz")
            .arg("-e")
            .arg(ssh_base_args(conn))
            .arg(remote)
            .arg(local);
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("rsync failed: {}", stderr.trim());
        }
        Ok(())
    } else {
        let mut cmd = Command::new("scp");
        cmd.arg("-P").arg(conn.port.to_string());
        cmd.arg(remote).arg(local);
        let output = cmd.output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("scp failed: {}", stderr.trim());
        }
        Ok(())
    }
}

/// Build the SSH base args string for rsync -e flag.
fn ssh_base_args(conn: &SshConnection) -> String {
    let mut args = format!("ssh -p {} -o StrictHostKeyChecking=accept-new", conn.port);
    if let crate::connection::SshAuth::KeyFile(ref path) = conn.auth {
        args.push_str(&format!(" -i {}", path));
    }
    args
}
