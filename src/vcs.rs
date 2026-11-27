//! VCS abstraction and the GitHub implementation.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// How to merge a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum MergeMethod {
    #[default]
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeMethod::Merge => "merge",
            MergeMethod::Squash => "squash",
            MergeMethod::Rebase => "rebase",
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
    /// Merges the PR, but only if its head is still `sha`.
    async fn merge(&self, repo: &str, number: u64, sha: &str, method: MergeMethod) -> Result<()>;
    /// `Authorization` header value git should send when fetching `repo`, if any.
    async fn git_auth_header(&self, repo: &str) -> Result<Option<String>>;
    /// Strings that must never appear in comments or logs.
    fn secrets(&self) -> Vec<String>;
    /// Marks the start of work on `repo`. Every call must be paired with [`Vcs::job_finished`].
    fn job_started(&self, _repo: &str) {}
    /// Marks the end of work on `repo`. Implementations may revoke short-lived credentials for
    /// `repo` once no job is using them.
    async fn job_finished(&self, _repo: &str) {}
}

pub struct GitHub {
    http: reqwest::Client,
    api: String,
    auth: Auth,
}

/// How imulup authenticates to GitHub.
pub enum Credentials {
    /// A personal access token (or any other long-lived token).
    Token(String),
    /// A GitHub App: imulup signs a JWT with the app's private key and exchanges it for
    /// short-lived installation tokens, scoped to one repo and the permissions imulup needs.
    App {
        app_id: u64,
        private_key_pem: String,
    },
}

enum Auth {
    Token(String),
    App(Box<AppAuth>),
}

struct AppAuth {
    app_id: u64,
    key: jsonwebtoken::EncodingKey,
    /// Lowercased `owner/repo` -> installation id.
    installations: Mutex<HashMap<String, u64>>,
    /// Lowercased `owner/repo` and scope -> token.
    tokens: Mutex<HashMap<(String, Scope), InstallationToken>>,
    /// Lowercased `owner/repo` -> number of jobs in progress.
    jobs: Mutex<HashMap<String, usize>>,
    /// Serializes minting, so concurrent callers share one token instead of racing to replace
    /// each other's (and losing track of one that then can't be revoked or redacted).
    minting: tokio::sync::Mutex<()>,
}

/// What an installation token is for. Each scope gets its own token with only the permissions it
/// needs, so the token handed to git can't comment, approve, or set statuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Scope {
    Api,
    Git,
    /// Only minted when imulup merges a PR (automerge), so other tokens stay read-only on code.
    Merge,
}

impl Scope {
    /// Installation token permissions. `metadata: read` is always included by GitHub.
    fn permissions(self) -> Value {
        match self {
            // Pull requests, files, and reviews; comments and reactions; commit statuses.
            Scope::Api => json!({
                "pull_requests": "write",
                "issues": "write",
                "statuses": "write",
            }),
            Scope::Git => json!({"contents": "read"}),
            Scope::Merge => json!({"contents": "write", "pull_requests": "write"}),
        }
    }
}

#[derive(Clone)]
struct InstallationToken {
    token: String,
    fetched: Instant,
    /// The token this one replaced. Kept for redaction: output from a job that started before
    /// the refresh can still contain it.
    previous: Option<String>,
}

/// Installation tokens last an hour; refresh well before that so long jobs never hold a dead one.
const TOKEN_REFRESH_AFTER: Duration = Duration::from_secs(45 * 60);

#[derive(Deserialize)]
struct GhInstallation {
    id: u64,
}
#[derive(Deserialize)]
struct GhAccessToken {
    token: String,
}
#[derive(Deserialize)]
struct GhApp {
    slug: String,
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
    pub fn new(api: &str, credentials: Credentials) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("imulup/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()?;
        let auth = match credentials {
            Credentials::Token(t) => Auth::Token(t),
            Credentials::App {
                app_id,
                private_key_pem,
            } => Auth::App(Box::new(AppAuth {
                app_id,
                key: jsonwebtoken::EncodingKey::from_rsa_pem(private_key_pem.as_bytes())
                    .context("GitHub App private key is not a valid RSA PEM key")?,
                installations: Mutex::default(),
                tokens: Mutex::default(),
                jobs: Mutex::default(),
                minting: tokio::sync::Mutex::default(),
            })),
        };
        Ok(Self {
            http,
            api: api.trim_end_matches('/').to_string(),
            auth,
        })
    }

    /// For a GitHub App, checks the credentials and returns the app's bot login
    /// (`<slug>[bot]`). Returns `None` for token auth.
    pub async fn app_login(&self) -> Result<Option<String>> {
        let Auth::App(app) = &self.auth else {
            return Ok(None);
        };
        let jwt = app.jwt()?;
        let a: GhApp = self
            .ok_with(&jwt, Method::GET, "/app", None)
            .await
            .context("authenticating as the GitHub App")?;
        Ok(Some(format!("{}[bot]", a.slug)))
    }

    /// The token to use for `scope` against `repo`.
    async fn token(&self, repo: &str, scope: Scope) -> Result<String> {
        let app = match &self.auth {
            Auth::Token(t) => return Ok(t.clone()),
            Auth::App(app) => app,
        };
        let key = repo.to_ascii_lowercase();
        let cache_key = (key.clone(), scope);
        let fresh = |t: &InstallationToken| t.fetched.elapsed() < TOKEN_REFRESH_AFTER;
        if let Some(t) = lock(&app.tokens).get(&cache_key).filter(|t| fresh(t)) {
            return Ok(t.token.clone());
        }
        let _minting = app.minting.lock().await;
        // Someone may have minted while we waited.
        let previous = match lock(&app.tokens).get(&cache_key) {
            Some(t) if fresh(t) => return Ok(t.token.clone()),
            Some(t) => Some(t.token.clone()),
            None => None,
        };

        let cached = lock(&app.installations).get(&key).copied();
        let installation = if let Some(id) = cached {
            id
        } else {
            let jwt = app.jwt()?;
            let (status, value) = self
                .request_with(
                    &jwt,
                    Method::GET,
                    &format!("/repos/{repo}/installation"),
                    None,
                )
                .await?;
            if status == StatusCode::NOT_FOUND {
                bail!("the GitHub App is not installed on {repo}");
            }
            if !status.is_success() {
                bail!("looking up the GitHub App installation for {repo} returned {status}");
            }
            let i: GhInstallation = serde_json::from_value(value)?;
            lock(&app.installations).insert(key.clone(), i.id);
            i.id
        };

        let name = repo.split_once('/').map_or(repo, |(_, name)| name);
        let body = json!({"repositories": [name], "permissions": scope.permissions()});
        let jwt = app.jwt()?;
        let path = format!("/app/installations/{installation}/access_tokens");
        let (status, value) = self
            .request_with(&jwt, Method::POST, &path, Some(body))
            .await?;
        if !status.is_success() {
            // The app may have been reinstalled under a new id; look it up again next time.
            lock(&app.installations).remove(&key);
            bail!(
                "creating an installation token for {repo} returned {status} \
                 (does the GitHub App have the permissions listed in the README?)"
            );
        }
        let t: GhAccessToken = serde_json::from_value(value)?;
        lock(&app.tokens).insert(
            cache_key,
            InstallationToken {
                token: t.token.clone(),
                fetched: Instant::now(),
                previous,
            },
        );
        Ok(t.token)
    }

    async fn request(
        &self,
        repo: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value)> {
        let token = self.token(repo, Scope::Api).await?;
        self.request_with(&token, method, path, body).await
    }

    async fn request_with(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<(StatusCode, Value)> {
        let url = format!("{}{}", self.api, path);
        let mut req = self
            .http
            .request(method.clone(), &url)
            .bearer_auth(token)
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
        repo: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let token = self.token(repo, Scope::Api).await?;
        self.ok_with(&token, method, path, body).await
    }

    async fn ok_with<T: for<'de> Deserialize<'de>>(
        &self,
        token: &str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T> {
        let (status, value) = self.request_with(token, method.clone(), path, body).await?;
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
        repo: &str,
        path: &str,
        max_pages: usize,
    ) -> Result<Vec<T>> {
        let mut out = Vec::new();
        for page in 1..=max_pages {
            let sep = if path.contains('?') { '&' } else { '?' };
            let items: Vec<T> = self
                .ok(
                    repo,
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

impl AppAuth {
    /// A short-lived JWT identifying the app, per
    /// <https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-a-json-web-token-jwt-for-a-github-app>.
    fn jwt(&self) -> Result<String> {
        #[derive(serde::Serialize)]
        struct Claims {
            iat: u64,
            exp: u64,
            iss: String,
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let claims = Claims {
            // Backdated to tolerate clock drift between us and GitHub.
            iat: now.saturating_sub(60),
            exp: now + 9 * 60,
            iss: self.app_id.to_string(),
        };
        let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
        Ok(jsonwebtoken::encode(&header, &claims, &self.key)?)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn basic_auth(token: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"))
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
        let mut pull: GhPull = self.ok(repo, Method::GET, &path, None).await?;
        // GitHub computes mergeability lazily; give it a moment if we asked first.
        if pull.mergeable.is_none() && pull.state == "open" {
            tokio::time::sleep(Duration::from_secs(2)).await;
            pull = self.ok(repo, Method::GET, &path, None).await?;
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
            .paginate(repo, &format!("/repos/{repo}/pulls/{number}/files"), 30)
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
                repo,
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
                repo,
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
                repo,
                Method::POST,
                &format!("/repos/{repo}/statuses/{sha}"),
                Some(body),
            )
            .await?;
        Ok(())
    }

    async fn is_approved(&self, repo: &str, number: u64, author: &str) -> Result<bool> {
        let reviews: Vec<GhReview> = self
            .paginate(repo, &format!("/repos/{repo}/pulls/{number}/reviews"), 10)
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

    async fn merge(&self, repo: &str, number: u64, sha: &str, method: MergeMethod) -> Result<()> {
        let token = self.token(repo, Scope::Merge).await?;
        let body = json!({"sha": sha, "merge_method": method.as_str()});
        let (status, value) = self
            .request_with(
                &token,
                Method::PUT,
                &format!("/repos/{repo}/pulls/{number}/merge"),
                Some(body),
            )
            .await?;
        if status.is_success() {
            return Ok(());
        }
        // 405: not mergeable (branch protection, conflicts); 409: the head moved.
        let reason = value["message"].as_str().map_or_else(
            || truncate(&value.to_string(), 300).to_string(),
            String::from,
        );
        bail!("GitHub refused the merge ({status}): {reason}");
    }

    async fn has_write_access(&self, repo: &str, user: &str) -> Result<bool> {
        let (status, value) = self
            .request(
                repo,
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

    async fn git_auth_header(&self, repo: &str) -> Result<Option<String>> {
        let token = self.token(repo, Scope::Git).await?;
        Ok(Some(format!("Basic {}", basic_auth(&token))))
    }

    fn secrets(&self) -> Vec<String> {
        let tokens: Vec<String> = match &self.auth {
            Auth::Token(t) => vec![t.clone()],
            Auth::App(app) => lock(&app.tokens)
                .values()
                .flat_map(|t| std::iter::once(t.token.clone()).chain(t.previous.clone()))
                .collect(),
        };
        tokens
            .into_iter()
            .flat_map(|t| {
                let basic = basic_auth(&t);
                [t, basic]
            })
            .collect()
    }

    fn job_started(&self, repo: &str) {
        if let Auth::App(app) = &self.auth {
            *lock(&app.jobs)
                .entry(repo.to_ascii_lowercase())
                .or_default() += 1;
        }
    }

    async fn job_finished(&self, repo: &str) {
        let Auth::App(app) = &self.auth else { return };
        let key = repo.to_ascii_lowercase();
        let revoke: Vec<String> = {
            // Hold `jobs` while evicting, so a job that starts now mints fresh tokens instead
            // of picking up ones that are about to be revoked.
            let mut jobs = lock(&app.jobs);
            let n = jobs.entry(key.clone()).or_default();
            *n = n.saturating_sub(1);
            if *n > 0 {
                return;
            }
            jobs.remove(&key);
            let mut tokens = lock(&app.tokens);
            [Scope::Api, Scope::Git, Scope::Merge]
                .into_iter()
                .filter_map(|scope| tokens.remove(&(key.clone(), scope)))
                .flat_map(|t| std::iter::once(t.token).chain(t.previous))
                .collect()
        };
        for token in revoke {
            match self
                .request_with(&token, Method::DELETE, "/installation/token", None)
                .await
            {
                // 401: already expired or revoked.
                Ok((s, _)) if s.is_success() || s == StatusCode::UNAUTHORIZED => {}
                Ok((s, _)) => tracing::warn!(%repo, "revoking installation token returned {s}"),
                Err(e) => tracing::warn!(%repo, "revoking installation token: {e:#}"),
            }
        }
    }
}
