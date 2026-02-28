use anyhow::{Context as _, anyhow};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug)]
pub struct Ssh {
    pub target: String,
    pub identity: Option<String>,
    pub port: Option<u16>,
}

impl Ssh {
    // Runs a single remote command via the system "ssh" binary and returns stdout.
    // This is intentionally simple: no interactive prompts, short connect timeout.
    pub async fn run(&self, remote_cmd: &str) -> anyhow::Result<String> {
        let mut cmd = Command::new("ssh");

        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("ConnectTimeout=5");

        if let Some(port) = self.port {
            cmd.arg("-p").arg(port.to_string());
        }

        if let Some(identity) = &self.identity {
            cmd.arg("-i").arg(identity);
        }

        cmd.arg(&self.target).arg("--").arg(remote_cmd);

        let out = cmd
            .stdin(Stdio::null())
            .output()
            .await
            .context("failed to start ssh")?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(anyhow!(
                "ssh failed: {}",
                if stderr.is_empty() {
                    "<no stderr>"
                } else {
                    &stderr
                }
            ));
        }

        String::from_utf8(out.stdout).context("ssh stdout was not valid UTF-8")
    }
}
