use crate::ssh::Ssh;
use anyhow::{Context as _, anyhow};
use std::path::Path;
use std::process::Stdio;
use tokio::process::{Child, Command};

#[derive(Clone, Debug)]
pub enum Runner {
    Ssh(Ssh),
    Local,
}

impl Runner {
    pub fn key(&self) -> String {
        match self {
            Runner::Ssh(ssh) => ssh.target.clone(),
            Runner::Local => "local".to_string(),
        }
    }

    pub async fn run(&self, cmd: &str) -> anyhow::Result<String> {
        match self {
            Runner::Ssh(ssh) => ssh.run(cmd).await,
            Runner::Local => {
                let out = Command::new("sh")
                    .arg("-lc")
                    .arg(cmd)
                    .stdin(Stdio::null())
                    .output()
                    .await
                    .context("failed to run local shell")?;

                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    return Err(anyhow!(
                        "local command failed: {}",
                        if stderr.is_empty() {
                            "<no stderr>"
                        } else {
                            &stderr
                        }
                    ));
                }
                Ok(String::from_utf8(out.stdout).context("local stdout was not valid UTF-8")?)
            }
        }
    }

    pub async fn copy_file_to(&self, local: &Path, remote_path: &str) -> anyhow::Result<()> {
        match self {
            Runner::Ssh(ssh) => {
                let mut cmd = Command::new("scp");
                cmd.arg("-o").arg("BatchMode=yes");
                cmd.arg("-o").arg("ConnectTimeout=10");
                if let Some(port) = ssh.port {
                    cmd.arg("-P").arg(port.to_string());
                }
                if let Some(identity) = &ssh.identity {
                    cmd.arg("-i").arg(identity);
                }
                cmd.arg(local);
                cmd.arg(format!("{}:{}", ssh.target, remote_path));
                let out = cmd
                    .stdin(Stdio::null())
                    .output()
                    .await
                    .context("failed to start scp")?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    return Err(anyhow!(
                        "scp failed: {}",
                        if stderr.is_empty() {
                            "<no stderr>"
                        } else {
                            &stderr
                        }
                    ));
                }
                Ok(())
            }
            Runner::Local => {
                let dst = Path::new(remote_path);
                let parent = dst
                    .parent()
                    .context("invalid destination path")?
                    .to_path_buf();
                std::fs::create_dir_all(&parent)
                    .context("failed to create destination directory")?;
                std::fs::copy(local, dst).context("failed to copy file")?;
                Ok(())
            }
        }
    }

    pub fn spawn_killable(&self, cmd: &str) -> anyhow::Result<Child> {
        match self {
            Runner::Ssh(ssh) => {
                let mut c = Command::new("ssh");
                c.arg("-o")
                    .arg("BatchMode=yes")
                    .arg("-o")
                    .arg("ConnectTimeout=5")
                    .arg("-o")
                    .arg("ServerAliveInterval=2")
                    .arg("-o")
                    .arg("ServerAliveCountMax=2");

                if let Some(port) = ssh.port {
                    c.arg("-p").arg(port.to_string());
                }
                if let Some(identity) = &ssh.identity {
                    c.arg("-i").arg(identity);
                }

                c.arg(&ssh.target).arg("--").arg(cmd);
                c.stdin(Stdio::null());
                c.stdout(Stdio::piped());
                c.stderr(Stdio::piped());
                Ok(c.spawn().context("failed to spawn ssh")?)
            }
            Runner::Local => {
                let mut c = Command::new("sh");
                c.arg("-lc").arg(cmd);
                c.stdin(Stdio::null());
                c.stdout(Stdio::piped());
                c.stderr(Stdio::piped());
                Ok(c.spawn().context("failed to spawn local shell")?)
            }
        }
    }
}
