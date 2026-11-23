//! Orchestration: turns webhook events into plans, applies, and lock changes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::sync::Mutex as AsyncMutex;
use tracing::{error, info, warn};

use crate::command::{self, CommandKind, Parsed, ProjectFilter};
use crate::config::ServerConfig;
use crate::render::{self, Outcome, ProjectResult};
use crate::repo_config::{self, Project, RepoConfig, Requirement, normalize};
use crate::runner::{self, StepEnv};
use crate::state::{Lock, PlanRecord, Store, now};
use crate::vcs::{PullRequest, StatusState, Vcs};
use crate::workspace::Workspaces;

pub const PLAN_STATUS: &str = "imulup/plan";
pub const APPLY_STATUS: &str = "imulup/apply";

#[derive(Debug, Clone)]
pub enum Event {
    PullUpdated {
        repo: String,
        number: u64,
    },
    PullClosed {
        repo: String,
        number: u64,
    },
    Comment {
        repo: String,
        number: u64,
        user: String,
        body: String,
        comment_id: u64,
    },
}

pub struct App {
    pub cfg: ServerConfig,
    pub vcs: Arc<dyn Vcs>,
    pub store: Store,
    pub workspaces: Workspaces,
    /// Serializes all work on a given PR.
    pr_locks: std::sync::Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

struct Checkout {
    root: PathBuf,
    config: Option<RepoConfig>,
    modified: Vec<String>,
}

impl App {
    pub fn new(cfg: ServerConfig, vcs: Arc<dyn Vcs>, store: Store, workspaces: Workspaces) -> Self {
        Self {
            cfg,
            vcs,
            store,
            workspaces,
            pr_locks: std::sync::Mutex::default(),
        }
    }

    fn pr_mutex(&self, repo: &str, number: u64) -> Arc<AsyncMutex<()>> {
        let mut m = self
            .pr_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.entry(format!("{repo}#{number}")).or_default().clone()
    }

    fn secrets(&self) -> Vec<String> {
        let mut s = self.vcs.secrets();
        s.push(self.cfg.gh_webhook_secret.clone());
        s
    }

    pub async fn handle(&self, event: Event) {
        let (repo, number) = match &event {
            Event::PullUpdated { repo, number }
            | Event::PullClosed { repo, number }
            | Event::Comment { repo, number, .. } => (repo.clone(), *number),
        };
        let mutex = self.pr_mutex(&repo, number);
        let guard = mutex.lock().await;
        let result = match event {
            Event::PullUpdated { .. } => self.autoplan(&repo, number).await,
            Event::PullClosed { .. } => self.closed(&repo, number).await,
            Event::Comment {
                user,
                body,
                comment_id,
                ..
            } => self.comment(&repo, number, &user, &body, comment_id).await,
        };
        if let Err(e) = result {
            error!(%repo, number, "{e:#}");
            let body = render::error(None, &format!("{e:#}"), &self.secrets());
            if let Err(e2) = self.vcs.comment(&repo, number, &body).await {
                error!(%repo, number, "also failed to comment: {e2:#}");
            }
        }
        drop(guard);
        // Drop the per-PR mutex once nobody else holds or waits on it. Waiters clone the Arc
        // under the same std lock, so the count can't change underneath us.
        let mut m = self
            .pr_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = format!("{repo}#{number}");
        if m.get(&key).is_some_and(|e| Arc::strong_count(e) == 2) {
            m.remove(&key);
        }
    }

    async fn checkout(&self, pull: &PullRequest) -> Result<Checkout> {
        let auth = self.vcs.git_auth_header(&pull.repo).await?;
        let root = self
            .workspaces
            .checkout(
                &pull.repo,
                pull.number,
                &pull.clone_url,
                &pull.head_sha,
                auth.as_deref(),
            )
            .await?;
        let config = repo_config::load(&root)?;
        let modified = self.vcs.modified_files(&pull.repo, pull.number).await?;
        Ok(Checkout {
            root,
            config,
            modified,
        })
    }

    async fn status(&self, pull: &PullRequest, state: StatusState, ctx: &str, desc: &str) {
        if let Err(e) = self
            .vcs
            .set_status(&pull.repo, &pull.head_sha, state, ctx, desc)
            .await
        {
            warn!(repo = %pull.repo, "setting {ctx} status: {e:#}");
        }
    }

    async fn autoplan(&self, repo: &str, number: u64) -> Result<()> {
        if self.cfg.disable_autoplan {
            return Ok(());
        }
        let pull = self.vcs.get_pull(repo, number).await?;
        if !pull.open || (pull.draft && !self.cfg.plan_drafts) {
            return Ok(());
        }
        if pull.is_fork && !self.cfg.allow_fork_prs {
            info!(repo, number, "skipping autoplan for fork PR");
            return Ok(());
        }
        let co = match self.checkout(&pull).await {
            Ok(c) => c,
            Err(e) => {
                self.status(&pull, StatusState::Failure, PLAN_STATUS, "setup failed")
                    .await;
                return Err(e);
            }
        };
        let projects: Vec<Project> =
            repo_config::modified_projects(&co.root, co.config.as_ref(), &co.modified)?
                .into_iter()
                .filter(|p| p.autoplan)
                .collect();
        if projects.is_empty() {
            info!(repo, number, "no Pulumi projects modified");
            return Ok(());
        }
        let author = pull.author.clone();
        self.plan(&pull, &co, &projects, &author).await
    }

    async fn closed(&self, repo: &str, number: u64) -> Result<()> {
        let released = self.store.release_pull(repo, number)?;
        self.workspaces.delete(repo, number).await?;
        if !released.is_empty() {
            let list: Vec<String> = released
                .iter()
                .map(|l| format!("* `{}`", l.project))
                .collect();
            self.vcs
                .comment(
                    repo,
                    number,
                    &format!(
                        "PR closed; imulup released these locks:\n\n{}",
                        list.join("\n")
                    ),
                )
                .await?;
        }
        Ok(())
    }

    async fn comment(
        &self,
        repo: &str,
        number: u64,
        user: &str,
        body: &str,
        comment_id: u64,
    ) -> Result<()> {
        if self
            .cfg
            .gh_user
            .as_deref()
            .is_some_and(|me| user.eq_ignore_ascii_case(me))
        {
            return Ok(());
        }
        let name = &self.cfg.command_name;
        let (kind, filter) = match command::parse(body, name) {
            Parsed::NotForUs => return Ok(()),
            Parsed::Help => return self.vcs.comment(repo, number, &command::usage(name)).await,
            Parsed::Error(msg) => {
                let body = format!(
                    "{}\n\n{}",
                    render::error(None, &msg, &[]),
                    command::usage(name)
                );
                return self.vcs.comment(repo, number, &body).await;
            }
            Parsed::Command { kind, filter } => (kind, filter),
        };
        if let Err(e) = self.vcs.react(repo, comment_id, "eyes").await {
            warn!(repo, "reacting to comment: {e:#}");
        }

        let pull = self.vcs.get_pull(repo, number).await?;
        let refuse = |msg: String| async move {
            let body = render::error(Some(kind), &msg, &[]);
            self.vcs.comment(repo, number, &body).await
        };
        if !pull.open {
            return refuse("this PR is closed".into()).await;
        }
        if pull.is_fork && !self.cfg.allow_fork_prs {
            return refuse("imulup does not run on PRs from forks".into()).await;
        }
        if !self.cfg.skip_collaborator_check && !self.vcs.has_write_access(repo, user).await? {
            return refuse(format!(
                "{user} needs write access to {repo} to run imulup commands"
            ))
            .await;
        }

        match kind {
            CommandKind::Unlock => {
                let released = self.store.release_pull(repo, number)?;
                let msg = if released.is_empty() {
                    "This PR held no locks.".to_string()
                } else {
                    let list: Vec<String> = released
                        .iter()
                        .map(|l| format!("* `{}`", l.project))
                        .collect();
                    format!("Released locks and discarded plans:\n\n{}", list.join("\n"))
                };
                self.status(
                    &pull,
                    StatusState::Failure,
                    PLAN_STATUS,
                    "plans discarded by unlock",
                )
                .await;
                self.vcs.comment(repo, number, &msg).await
            }
            CommandKind::Plan => {
                let co = self.checkout(&pull).await?;
                let projects = select(&co, &filter)?;
                if projects.is_empty() {
                    return refuse(
                        "no Pulumi projects matched (nothing modified, or no such dir/stack)"
                            .into(),
                    )
                    .await;
                }
                self.plan(&pull, &co, &projects, user).await
            }
            CommandKind::Apply => {
                let co = self.checkout(&pull).await?;
                let projects = if filter.is_empty() {
                    self.pending_applies(&pull, &co)
                } else {
                    select(&co, &filter)?
                };
                if projects.is_empty() {
                    return refuse(format!("nothing to apply; run `{name} plan` first")).await;
                }
                self.apply(&pull, &co, &projects, user).await
            }
        }
    }

    /// Projects with a successful, not-yet-applied plan on this PR.
    fn pending_applies(&self, pull: &PullRequest, co: &Checkout) -> Vec<Project> {
        let plans = self.store.plans_for_pull(&pull.repo, pull.number);
        plans
            .into_iter()
            .filter(|p| p.succeeded && !p.applied)
            .filter_map(|plan| {
                repo_config::all_projects(&co.root, co.config.as_ref(), Some(&plan.dir))
                    .into_iter()
                    .find(|p| p.dir == plan.dir && p.stack == plan.stack)
            })
            .collect()
    }

    fn step_env(&self, pull: &PullRequest, p: &Project, user: &str) -> StepEnv {
        StepEnv {
            pulumi_bin: self.cfg.pulumi_bin.clone(),
            stack: p.stack.clone(),
            project: p.id(),
            dir: p.dir.clone(),
            repo: pull.repo.clone(),
            pull: pull.number,
            head_sha: pull.head_sha.clone(),
            user: user.to_string(),
            timeout: Duration::from_secs(self.cfg.step_timeout_secs),
        }
    }

    fn acquire(&self, pull: &PullRequest, p: &Project, user: &str) -> Result<Option<String>> {
        let lock = Lock {
            repo: pull.repo.clone(),
            dir: p.dir.clone(),
            stack: p.stack.clone(),
            project: p.id(),
            pull: pull.number,
            user: user.to_string(),
            created_at: now(),
        };
        Ok(match self.store.try_lock(lock)? {
            Ok(()) => None,
            Err(other) => Some(format!(
                "`{}` is locked by #{} (locked by {}). Merge or close that PR, or comment `{} unlock` on it.",
                p.id(),
                other.pull,
                other.user,
                self.cfg.command_name
            )),
        })
    }

    async fn plan(
        &self,
        pull: &PullRequest,
        co: &Checkout,
        projects: &[Project],
        user: &str,
    ) -> Result<()> {
        self.status(
            pull,
            StatusState::Pending,
            PLAN_STATUS,
            &format!("planning {} project(s)", projects.len()),
        )
        .await;
        let mut results = Vec::new();
        for p in projects {
            let outcome = if let Some(reason) = self.acquire(pull, p, user)? {
                Outcome::Skipped { reason }
            } else {
                let dir = co.root.join(&p.dir);
                if dir.is_dir() {
                    let r = runner::run_steps(
                        &p.workflow.plan.steps,
                        &dir,
                        &self.step_env(pull, p, user),
                    )
                    .await;
                    let has_changes = if r.success {
                        runner::preview_has_changes(&r.output)
                    } else {
                        None
                    };
                    self.store.put_plan(PlanRecord {
                        repo: pull.repo.clone(),
                        pull: pull.number,
                        project: p.id(),
                        dir: p.dir.clone(),
                        stack: p.stack.clone(),
                        head_sha: pull.head_sha.clone(),
                        succeeded: r.success,
                        has_changes,
                        applied: false,
                    })?;
                    if r.success {
                        Outcome::Success {
                            output: r.output,
                            has_changes,
                        }
                    } else {
                        Outcome::Failure { output: r.output }
                    }
                } else {
                    Outcome::Failure {
                        output: format!("directory `{}` does not exist", p.dir),
                    }
                }
            };
            results.push(ProjectResult {
                project: p.id(),
                selector: p.selector(),
                outcome,
            });
        }

        let body = render::results(
            CommandKind::Plan,
            &results,
            &self.cfg.command_name,
            &self.secrets(),
        );
        self.vcs.comment(&pull.repo, pull.number, &body).await?;
        let failed = results.iter().filter(|r| !r.ok()).count();
        if failed == 0 {
            self.status(
                pull,
                StatusState::Success,
                PLAN_STATUS,
                &format!("{} planned", results.len()),
            )
            .await;
        } else {
            self.status(
                pull,
                StatusState::Failure,
                PLAN_STATUS,
                &format!("{failed}/{} failed", results.len()),
            )
            .await;
        }
        self.update_apply_status(pull, false).await;
        Ok(())
    }

    async fn apply(
        &self,
        pull: &PullRequest,
        co: &Checkout,
        projects: &[Project],
        user: &str,
    ) -> Result<()> {
        let mut approved: Option<bool> = None;
        let mut results = Vec::new();
        for p in projects {
            let outcome = if let Some(reason) =
                self.check_apply(pull, p, user, &mut approved).await?
            {
                Outcome::Skipped { reason }
            } else {
                let dir = co.root.join(&p.dir);
                let r =
                    runner::run_steps(&p.workflow.apply.steps, &dir, &self.step_env(pull, p, user))
                        .await;
                if r.success {
                    self.store
                        .mark_applied(&pull.repo, pull.number, &p.dir, &p.stack)?;
                    Outcome::Success {
                        output: r.output,
                        has_changes: None,
                    }
                } else {
                    Outcome::Failure { output: r.output }
                }
            };
            results.push(ProjectResult {
                project: p.id(),
                selector: p.selector(),
                outcome,
            });
        }
        let body = render::results(
            CommandKind::Apply,
            &results,
            &self.cfg.command_name,
            &self.secrets(),
        );
        self.vcs.comment(&pull.repo, pull.number, &body).await?;
        let any_failed = results
            .iter()
            .any(|r| matches!(r.outcome, Outcome::Failure { .. }));
        self.update_apply_status(pull, any_failed).await;
        Ok(())
    }

    /// Returns why `p` can't be applied right now, if it can't.
    async fn check_apply(
        &self,
        pull: &PullRequest,
        p: &Project,
        user: &str,
        approved: &mut Option<bool>,
    ) -> Result<Option<String>> {
        let name = &self.cfg.command_name;
        let replan = format!("`{name} plan {}`", p.selector());
        let Some(plan) = self
            .store
            .get_plan(&pull.repo, pull.number, &p.dir, &p.stack)
        else {
            return Ok(Some(format!("no plan yet; run {replan}")));
        };
        if plan.head_sha != pull.head_sha {
            return Ok(Some(format!(
                "plan is for commit {} but the PR is at {}; run {replan}",
                short(&plan.head_sha),
                short(&pull.head_sha)
            )));
        }
        if !plan.succeeded {
            return Ok(Some(format!(
                "the last plan failed; fix it and run {replan}"
            )));
        }
        if plan.applied {
            return Ok(Some(format!(
                "already applied at this commit; run {replan} to apply again"
            )));
        }
        if let Some(reason) = self.acquire(pull, p, user)? {
            return Ok(Some(reason));
        }
        let mut reqs = p.apply_requirements.clone();
        if self.cfg.require_approval {
            reqs.push(Requirement::Approved);
        }
        if self.cfg.require_mergeable {
            reqs.push(Requirement::Mergeable);
        }
        for req in reqs {
            let ok = match req {
                Requirement::Approved => {
                    if let Some(a) = *approved {
                        a
                    } else {
                        let a = self
                            .vcs
                            .is_approved(&pull.repo, pull.number, &pull.author)
                            .await?;
                        *approved = Some(a);
                        a
                    }
                }
                Requirement::Mergeable => pull.mergeable == Some(true),
            };
            if !ok {
                return Ok(Some(format!(
                    "apply requirement `{}` is not met",
                    req.as_str()
                )));
            }
        }
        Ok(None)
    }

    async fn update_apply_status(&self, pull: &PullRequest, failed: bool) {
        let plans: Vec<PlanRecord> = self
            .store
            .plans_for_pull(&pull.repo, pull.number)
            .into_iter()
            .filter(|p| p.head_sha == pull.head_sha && p.succeeded)
            .collect();
        if plans.is_empty() {
            return;
        }
        let done = plans
            .iter()
            .filter(|p| p.applied || p.has_changes == Some(false))
            .count();
        let desc = format!("{done}/{} applied or unchanged", plans.len());
        let state = if failed {
            StatusState::Failure
        } else if done == plans.len() {
            StatusState::Success
        } else {
            StatusState::Pending
        };
        self.status(pull, state, APPLY_STATUS, &desc).await;
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

/// Resolves a comment's project filter against the checkout.
fn select(co: &Checkout, f: &ProjectFilter) -> Result<Vec<Project>> {
    if let Some(name) = &f.project {
        let Some(cfg) = &co.config else {
            bail!("--project needs an imulup.yaml that names projects; use --dir/--stack instead");
        };
        return match cfg
            .projects()
            .into_iter()
            .find(|p| p.name.as_deref() == Some(name))
        {
            Some(p) => Ok(vec![p]),
            None => bail!("no project named `{name}` in imulup.yaml"),
        };
    }
    let dir = f.dir.as_deref().and_then(normalize);
    let candidates = if f.is_empty() {
        repo_config::modified_projects(&co.root, co.config.as_ref(), &co.modified)?
    } else if dir.is_some() || co.config.is_some() {
        repo_config::all_projects(&co.root, co.config.as_ref(), dir.as_deref())
    } else {
        // Only --stack, no config: narrow the modified projects.
        repo_config::modified_projects(&co.root, None, &co.modified)?
    };
    Ok(candidates
        .into_iter()
        .filter(|p| dir.as_ref().is_none_or(|d| &p.dir == d))
        .filter(|p| f.stack.as_ref().is_none_or(|s| &p.stack == s))
        .collect())
}
