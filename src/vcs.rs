//! VCS abstraction and the GitHub implementation.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::Engine;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct PullRequest {
    pub repo: String,
    pub number: u64,
    pub head_sha: String,
    pub author: String,
    pub open: bool,
    pub draft: bool,
    pub is_fork: bool,
    /// `None` while GitHub is still computing it.
    pub mergeable: Option<bool>,
    /// Clone URL of the base repo (without credentials).
    pub clone_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusState {
    Pending,
    Success,
    Failure,
}

impl StatusState {
    fn as_str(self) -> &'static str {
        match self {
            StatusState::Pending => "pending",
            StatusState::Success => "success",
            StatusState::Failure => "failure",
        }
    }
}

#[async_trait]
pub trait Vcs: Send + Sync {
    async fn get_pull(&self, repo: &str, number: u64) -> Result<PullRequest>;
    /// Repo-relative paths changed by the PR (including the old path of renames).
    async fn modified_files(&self, repo: &str, number: u64) -> Result<Vec<String>>;
    async fn comment(&self, repo: &str, number: u64, body: &str) -> Result<()>;
    async fn react(&self, repo: &str, comment_id: u64, reaction: &str) -> Result<()>;
    async fn set_status(
        &self,
        repo: &str,
        sha: &str,
        state: StatusState,
        context: &str,
        description: &str,
    ) -> Result<()>;
    /// True if someone other than `author` has an outstanding approving review.
    async fn is_approved(&self, repo: &str, number: u64, author: &str) -> Result<bool>;
    async fn has_write_access(&self, repo: &str, user: &str) -> Result<bool>;
    /// `Authorization` header value git should send when fetching, if any.
    fn git_auth_header(&self) -> Option<String>;
    /// Strings that must never appear in comments or logs.
    fn secrets(&self) -> Vec<String>;
}

pub struct GitHub {
    http: reqwest::Client,
    api: String,
    token: String,
}

#[derive(Deserialize)]
struct GhUser {
    login: String,
}
#[derive(Deserialize)]
struct GhRepo {
    full_name: String,
    clone_url: String,
}
#[derive(Deserialize)]
struct GhRef {
    sha: String,
    repo: Option<GhRepo>,
}
#[derive(Deserialize)]
struct GhPull {
    number: u64,
    state: String,
    #[serde(default)]
    draft: bool,
    user: GhUser,
    head: GhRef,
    base: GhRef,
    mergeable: Option<bool>,
}
#[derive(Deserialize)]
struct GhFile {
    filename: String,
    previous_filename: Option<String>,
}
#[derive(Deserialize)]
struct GhReview {
    user: Option<GhUser>,
    state: String,
}

const PAGE: usize = 100;

impl GitHub {
    pub fn new(api: &str, token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("imulup/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            api: api.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value)> {
        let url = format!("{}{}", self.api, path);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req
            .send()
            .await
            .with_context(|| format!("{method} {path}"))?;
        let status = resp.status();
        let text = resp.text().await?;
        let value = if text.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok((status, value))
    }

    async fn ok<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let (status, value) = self.request(method.clone(), path, body).await?;
        if !status.is_success() {
            bail!(
                "GitHub {method} {path} returned {status}: {}",
                truncate(&value.to_string(), 500)
            );
        }
        Ok(serde_json::from_value(value)?)
    }

    async fn paginate<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        max_pages: usize,
    ) -> Result<Vec<T>> {
        let mut out = Vec::new();
        for page in 1..=max_pages {
            let sep = if path.contains('?') { '&' } else { '?' };
            let items: Vec<T> = self
                .ok(
                    Method::GET,
                    &format!("{path}{sep}per_page={PAGE}&page={page}"),
                    None,
                )
                .await?;
            let n = items.len();
            out.extend(items);
            if n < PAGE {
                break;
            }
        }
        Ok(out)
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[async_trait]
impl Vcs for GitHub {
    async fn get_pull(&self, repo: &str, number: u64) -> Result<PullRequest> {
        let path = format!("/repos/{repo}/pulls/{number}");
        let mut pull: GhPull = self.ok(Method::GET, &path, None).await?;
        // GitHub computes mergeability lazily; give it a moment if we asked first.
        if pull.mergeable.is_none() && pull.state == "open" {
            tokio::time::sleep(Duration::from_secs(2)).await;
            pull = self.ok(Method::GET, &path, None).await?;
        }
        let base = pull.base.repo.context("PR base repo missing")?;
        let is_fork = pull
            .head
            .repo
            .as_ref()
            .is_none_or(|r| r.full_name != base.full_name);
        Ok(PullRequest {
            repo: base.full_name,
            number: pull.number,
            head_sha: pull.head.sha,
            author: pull.user.login,
            open: pull.state == "open",
            draft: pull.draft,
            is_fork,
            mergeable: pull.mergeable,
            clone_url: base.clone_url,
        })
    }

    async fn modified_files(&self, repo: &str, number: u64) -> Result<Vec<String>> {
        // GitHub caps this endpoint at 3000 files.
        let files: Vec<GhFile> = self
            .paginate(&format!("/repos/{repo}/pulls/{number}/files"), 30)
            .await?;
        let mut out = Vec::new();
        for f in files {
            out.push(f.filename);
            out.extend(f.previous_filename);
        }
        Ok(out)
    }

    async fn comment(&self, repo: &str, number: u64, body: &str) -> Result<()> {
        let _: Value = self
            .ok(
                Method::POST,
                &format!("/repos/{repo}/issues/{number}/comments"),
                Some(json!({"body": body})),
            )
            .await?;
        Ok(())
    }

    async fn react(&self, repo: &str, comment_id: u64, reaction: &str) -> Result<()> {
        let _: Value = self
            .ok(
                Method::POST,
                &format!("/repos/{repo}/issues/comments/{comment_id}/reactions"),
                Some(json!({"content": reaction})),
            )
            .await?;
        Ok(())
    }

    async fn set_status(
        &self,
        repo: &str,
        sha: &str,
        state: StatusState,
        context: &str,
        description: &str,
    ) -> Result<()> {
        let body = json!({
            "state": state.as_str(),
            "context": context,
            "description": truncate(description, 140),
        });
        let _: Value = self
            .ok(
                Method::POST,
                &format!("/repos/{repo}/statuses/{sha}"),
                Some(body),
            )
            .await?;
        Ok(())
    }

    async fn is_approved(&self, repo: &str, number: u64, author: &str) -> Result<bool> {
        let reviews: Vec<GhReview> = self
            .paginate(&format!("/repos/{repo}/pulls/{number}/reviews"), 10)
            .await?;
        // Reviews come back oldest first; a reviewer's latest non-comment review is what counts.
        let mut latest: HashMap<String, String> = HashMap::new();
        for r in reviews {
            let Some(user) = r.user else { continue };
            if r.state != "COMMENTED" && !user.login.eq_ignore_ascii_case(author) {
                latest.insert(user.login, r.state);
            }
        }
        Ok(latest.values().any(|s| s == "APPROVED"))
    }

    async fn has_write_access(&self, repo: &str, user: &str) -> Result<bool> {
        let (status, value) = self
            .request(
                Method::GET,
                &format!("/repos/{repo}/collaborators/{user}/permission"),
                None,
            )
            .await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !status.is_success() {
            bail!("permission check for {user} returned {status}");
        }
        Ok(matches!(
            value["permission"].as_str(),
            Some("admin" | "maintain" | "write")
        ))
    }

    fn git_auth_header(&self) -> Option<String> {
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", self.token));
        Some(format!("Basic {basic}"))
    }

    fn secrets(&self) -> Vec<String> {
        let basic = base64::engine::general_purpose::STANDARD
            .encode(format!("x-access-token:{}", self.token));
        vec![self.token.clone(), basic]
    }
}
