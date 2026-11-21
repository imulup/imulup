//! Per-PR git checkouts under `<data_dir>/repos/<owner>/<repo>/<pull>`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

pub struct Workspaces {
    root: PathBuf,
}

impl Workspaces {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dir(&self, repo: &str, pull: u64) -> PathBuf {
        let safe = repo.replace("..", "_");
        self.root.join(safe).join(pull.to_string())
    }

    /// Checks out the PR's head commit. Reuses an existing checkout (keeping ignored files such
    /// as `node_modules`) and only fetches when the head moved.
    pub async fn checkout(
        &self,
        repo: &str,
        pull: u64,
        clone_url: &str,
        head_sha: &str,
        auth_header: Option<&str>,
    ) -> Result<PathBuf> {
        let dir = self.dir(repo, pull);
        if dir.join(".git").is_dir()
            && git(&dir, &["rev-parse", "HEAD"], None)
                .await
                .ok()
                .as_deref()
                == Some(head_sha)
        {
            git(&dir, &["clean", "-fd"], None).await?;
            git(&dir, &["checkout", "--force", "--detach", head_sha], None).await?;
            return Ok(dir);
        }
        if dir.join(".git").is_dir() {
            git(&dir, &["remote", "set-url", "origin", clone_url], None).await?;
        } else {
            tokio::fs::create_dir_all(&dir).await?;
            git(&dir, &["init", "--quiet"], None).await?;
            git(&dir, &["remote", "add", "origin", clone_url], None).await?;
        }
        let refspec = format!("+refs/pull/{pull}/head:refs/remotes/origin/pr");
        git(
            &dir,
            &[
                "fetch",
                "--quiet",
                "--force",
                "--depth=100",
                "origin",
                &refspec,
            ],
            auth_header,
        )
        .await
        .context("fetching PR head")?;
        git(
            &dir,
            &[
                "checkout",
                "--quiet",
                "--force",
                "--detach",
                "refs/remotes/origin/pr",
            ],
            None,
        )
        .await?;
        git(&dir, &["clean", "-fd"], None).await?;
        let actual = git(&dir, &["rev-parse", "HEAD"], None).await?;
        if actual != head_sha {
            bail!(
                "PR head moved while running (expected {head_sha}, found {actual}); the new push will be planned separately"
            );
        }
        Ok(dir)
    }

    pub async fn delete(&self, repo: &str, pull: u64) -> Result<()> {
        let dir = self.dir(repo, pull);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await?;
        }
        Ok(())
    }
}

async fn git(dir: &Path, args: &[&str], auth_header: Option<&str>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .kill_on_drop(true);
    // Pass credentials through env-based config so they never land in .git/config or argv.
    if let Some(h) = auth_header {
        cmd.env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "http.extraHeader")
            .env("GIT_CONFIG_VALUE_0", format!("Authorization: {h}"));
    }
    let out = cmd
        .output()
        .await
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
