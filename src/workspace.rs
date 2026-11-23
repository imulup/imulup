//! Per-PR git checkouts.
//!
//! Fetching happens in two stages so git credentials never reach a process that PR code can
//! inspect:
//!
//! 1. imulup fetches the PR head, with credentials, into a bare mirror at
//!    `<data_dir>/mirrors/<owner>/<repo>.git` that it owns.
//! 2. The step user fetches from that local mirror, without credentials, into the PR's checkout
//!    at `<data_dir>/repos/<owner>/<repo>/<pull>`, which it owns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;

use crate::privsep::Identity;

pub struct Workspaces {
    repos: PathBuf,
    mirrors: PathBuf,
    step_user: Option<Identity>,
    /// Serializes fetches into each mirror; concurrent fetches would fight over git's locks.
    mirror_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Workspaces {
    /// `step_user` owns the checkouts and runs every git command in them. `None` runs them as
    /// imulup's own user.
    pub fn new(data_dir: &Path, step_user: Option<Identity>) -> Self {
        Self {
            repos: data_dir.join("repos"),
            mirrors: data_dir.join("mirrors"),
            step_user,
            mirror_locks: Mutex::default(),
        }
    }

    pub fn step_user(&self) -> Option<&Identity> {
        self.step_user.as_ref()
    }

    pub fn dir(&self, repo: &str, pull: u64) -> PathBuf {
        self.repos.join(safe(repo)).join(pull.to_string())
    }

    fn mirror(&self, repo: &str) -> PathBuf {
        self.mirrors.join(format!("{}.git", safe(repo)))
    }

    fn mirror_lock(&self, repo: &str) -> Arc<AsyncMutex<()>> {
        let mut m = self
            .mirror_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.entry(repo.to_ascii_lowercase()).or_default().clone()
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
        let user = self.step_user.as_ref();
        if dir.join(".git").is_dir()
            && git(&dir, &["rev-parse", "HEAD"], None, user)
                .await
                .ok()
                .as_deref()
                == Some(head_sha)
        {
            git(&dir, &["clean", "-fd"], None, user).await?;
            git(
                &dir,
                &["checkout", "--force", "--detach", head_sha],
                None,
                user,
            )
            .await?;
            return Ok(dir);
        }

        let pr_ref = format!("refs/pull/{pull}/head");
        let mirror = self.mirror(repo);
        {
            let lock = self.mirror_lock(repo);
            let _guard = lock.lock().await;
            if !mirror.join("HEAD").is_file() {
                tokio::fs::create_dir_all(&mirror).await?;
                git(&mirror, &["init", "--bare", "--quiet"], None, None).await?;
            }
            // Fetch by URL rather than a configured remote, so nothing about the origin (let
            // alone credentials) is stored in the mirror's config.
            git(
                &mirror,
                &[
                    "fetch",
                    "--quiet",
                    "--force",
                    "--no-tags",
                    "--depth=100",
                    clone_url,
                    &format!("+{pr_ref}:{pr_ref}"),
                ],
                auth_header,
                None,
            )
            .await
            .context("fetching PR head")?;
        }

        if !dir.join(".git").is_dir() {
            if user.is_none() {
                // With a step user, privsep::prepare_data_dir creates this with its owner.
                tokio::fs::create_dir_all(&self.repos).await?;
            }
            // `git init <dir>` creates missing parents, owned by the step user.
            let dir_arg = dir.to_string_lossy();
            git(&self.repos, &["init", "--quiet", &dir_arg], None, user).await?;
        }
        let mirror_arg = mirror.to_string_lossy();
        // The mirror belongs to imulup's user, so git's ownership check would refuse to serve it
        // to the step user. `-c` doesn't reach the local upload-pack (git clears config from its
        // environment), so trust the mirror on upload-pack's own command line.
        let upload_pack = format!(
            "--upload-pack=git -c {} upload-pack",
            shell_words::quote(&format!("safe.directory={mirror_arg}"))
        );
        git(
            &dir,
            &[
                "fetch",
                &upload_pack,
                "--quiet",
                "--force",
                "--no-tags",
                "--depth=100",
                &mirror_arg,
                &format!("+{pr_ref}:refs/remotes/origin/pr"),
            ],
            None,
            user,
        )
        .await
        .context("fetching PR head from the local mirror")?;
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
            user,
        )
        .await?;
        git(&dir, &["clean", "-fd"], None, user).await?;
        let actual = git(&dir, &["rev-parse", "HEAD"], None, user).await?;
        if actual != head_sha {
            bail!(
                "PR head moved while running (expected {head_sha}, found {actual}); the new push will be planned separately"
            );
        }
        Ok(dir)
    }

    pub async fn delete(&self, repo: &str, pull: u64) -> Result<()> {
        let mirror = self.mirror(repo);
        if mirror.join("HEAD").is_file() {
            let lock = self.mirror_lock(repo);
            let _guard = lock.lock().await;
            let pr_ref = format!("refs/pull/{pull}/head");
            // Best effort: the ref may never have been fetched.
            let _ = git(&mirror, &["update-ref", "-d", &pr_ref], None, None).await;
        }
        let dir = self.dir(repo, pull);
        if !dir.exists() {
            return Ok(());
        }
        match &self.step_user {
            None => tokio::fs::remove_dir_all(&dir).await?,
            // The checkout belongs to the step user, so only it can remove what's inside.
            Some(user) => {
                let out = command("rm", Some(user))
                    .arg("-rf")
                    .arg("--")
                    .arg(&dir)
                    .output()
                    .await?;
                if !out.status.success() {
                    bail!(
                        "removing {}: {}",
                        dir.display(),
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                }
            }
        }
        Ok(())
    }
}

fn safe(repo: &str) -> String {
    repo.replace("..", "_")
}

/// A command that runs as `user`, with its home directory, or as imulup's user for `None`.
fn command(program: &str, user: Option<&Identity>) -> Command {
    let mut cmd = Command::new(program);
    cmd.kill_on_drop(true);
    if let Some(u) = user {
        cmd.uid(u.uid)
            .gid(u.gid)
            .env("HOME", &u.home)
            .env("USER", &u.name)
            .env("LOGNAME", &u.name);
    }
    cmd
}

async fn git(
    dir: &Path,
    args: &[&str],
    auth_header: Option<&str>,
    user: Option<&Identity>,
) -> Result<String> {
    let mut cmd = command("git", user);
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1");
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
        // Name the subcommand, skipping `-c <key=value>` options.
        let mut rest = args.iter();
        let mut sub = "";
        while let Some(a) = rest.next() {
            if *a == "-c" {
                rest.next();
            } else {
                sub = a;
                break;
            }
        }
        bail!(
            "git {sub} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
