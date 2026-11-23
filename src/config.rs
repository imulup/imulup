use std::path::PathBuf;

use anyhow::Context;
use clap::{ArgAction, ArgGroup, Parser, builder::BoolishValueParser};
use globset::{GlobBuilder, GlobMatcher};

/// Server configuration. Every flag can also be set via its `IMULUP_*` environment variable;
/// prefer env vars for secrets so they don't show up in `ps`.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "imulup",
    version,
    about = "Pulumi pull request automation (Atlantis-style)",
    // Authenticate with exactly one of a token or a GitHub App.
    group(ArgGroup::new("gh_auth").required(true).args(["gh_token", "gh_app_id"])),
    group(ArgGroup::new("gh_app_key_source").args(["gh_app_key", "gh_app_key_file"])),
)]
pub struct ServerConfig {
    /// Address to bind the HTTP server to.
    #[arg(long, env = "IMULUP_BIND", default_value = "0.0.0.0")]
    pub bind: String,

    /// Port to listen on.
    #[arg(long, env = "IMULUP_PORT", default_value_t = 4141)]
    pub port: u16,

    /// GitHub token (PAT) used for API calls and git fetches. Use this or `--gh-app-id`.
    #[arg(
        long,
        env = "IMULUP_GH_TOKEN",
        hide_env_values = true,
        requires = "gh_user"
    )]
    pub gh_token: Option<String>,

    /// GitHub App ID. imulup authenticates as the app and uses a short-lived installation token
    /// for each repo. Needs `--gh-app-key` or `--gh-app-key-file`.
    #[arg(long, env = "IMULUP_GH_APP_ID", requires = "gh_app_key_source")]
    pub gh_app_id: Option<u64>,

    /// GitHub App private key (PEM contents). Prefer `--gh-app-key-file`: environment variables
    /// are easier to leak.
    #[arg(long, env = "IMULUP_GH_APP_KEY", hide_env_values = true)]
    pub gh_app_key: Option<String>,

    /// Path to the GitHub App private key (PEM file). It is read once at startup and must not be
    /// readable by group or others.
    #[arg(long, env = "IMULUP_GH_APP_KEY_FILE")]
    pub gh_app_key_file: Option<PathBuf>,

    /// Secret configured on the GitHub webhook; used to verify X-Hub-Signature-256.
    #[arg(long, env = "IMULUP_GH_WEBHOOK_SECRET", hide_env_values = true)]
    pub gh_webhook_secret: String,

    /// Login imulup acts as; comments from it are ignored. Required with `--gh-token`. With a
    /// GitHub App it defaults to `<app-slug>[bot]`.
    #[arg(long, env = "IMULUP_GH_USER")]
    pub gh_user: Option<String>,

    /// GitHub API base URL (set for GitHub Enterprise, e.g. <https://ghe.example.com/api/v3>).
    #[arg(
        long,
        env = "IMULUP_GH_API_URL",
        default_value = "https://api.github.com"
    )]
    pub gh_api_url: String,

    /// Comma-separated repo globs imulup will act on, e.g. `my-org/*,other-org/infra`.
    #[arg(
        long,
        env = "IMULUP_REPO_ALLOWLIST",
        value_delimiter = ',',
        required = true
    )]
    pub repo_allowlist: Vec<String>,

    /// Directory for clones and lock/plan state.
    #[arg(long, env = "IMULUP_DATA_DIR", default_value = "./imulup-data")]
    pub data_dir: PathBuf,

    /// Path to the pulumi binary.
    #[arg(long, env = "IMULUP_PULUMI_BIN", default_value = "pulumi")]
    pub pulumi_bin: String,

    /// Word that prefixes PR comment commands (`imulup plan`).
    #[arg(long, env = "IMULUP_COMMAND_NAME", default_value = "imulup")]
    pub command_name: String,

    /// Run on PRs from forks. Pulumi programs are arbitrary code: only enable for trusted forks.
    #[arg(long, env = "IMULUP_ALLOW_FORK_PRS", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub allow_fork_prs: bool,

    /// Disable automatic `plan` when PRs are opened or pushed to.
    #[arg(long, env = "IMULUP_DISABLE_AUTOPLAN", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub disable_autoplan: bool,

    /// Also autoplan draft PRs.
    #[arg(long, env = "IMULUP_PLAN_DRAFTS", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub plan_drafts: bool,

    /// Require PR approval before any apply (in addition to per-project requirements).
    #[arg(long, env = "IMULUP_REQUIRE_APPROVAL", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub require_approval: bool,

    /// Require the PR to be mergeable (no conflicts) before any apply.
    #[arg(long, env = "IMULUP_REQUIRE_MERGEABLE", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub require_mergeable: bool,

    /// Let anyone who can comment run commands. By default commenters need write access.
    #[arg(long, env = "IMULUP_SKIP_COLLABORATOR_CHECK", action = ArgAction::SetTrue, value_parser = BoolishValueParser::new())]
    pub skip_collaborator_check: bool,

    /// Maximum seconds a single workflow step may run.
    #[arg(long, env = "IMULUP_STEP_TIMEOUT_SECS", default_value_t = 3600)]
    pub step_timeout_secs: u64,
}

impl ServerConfig {
    /// The GitHub credentials selected by the flags, with the app key file read in.
    pub fn gh_credentials(&self) -> anyhow::Result<crate::vcs::Credentials> {
        use crate::vcs::Credentials;
        if let Some(token) = &self.gh_token {
            return Ok(Credentials::Token(token.clone()));
        }
        let app_id = self.gh_app_id.context("set --gh-token or --gh-app-id")?;
        let private_key_pem = match (&self.gh_app_key, &self.gh_app_key_file) {
            (Some(k), _) => k.clone(),
            (None, Some(path)) => {
                crate::hardening::check_key_file_permissions(path)
                    .context("unsafe --gh-app-key-file")?;
                std::fs::read_to_string(path)
                    .with_context(|| format!("reading GitHub App key {}", path.display()))?
            }
            (None, None) => anyhow::bail!("--gh-app-id needs --gh-app-key or --gh-app-key-file"),
        };
        Ok(Credentials::App {
            app_id,
            private_key_pem,
        })
    }

    pub fn repo_allowlist_matcher(&self) -> anyhow::Result<RepoAllowlist> {
        RepoAllowlist::new(&self.repo_allowlist)
    }
}

#[derive(Debug, Clone)]
pub struct RepoAllowlist(Vec<GlobMatcher>);

impl RepoAllowlist {
    pub fn new(patterns: &[String]) -> anyhow::Result<Self> {
        let mut out = Vec::new();
        for p in patterns.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            let glob = GlobBuilder::new(p)
                .case_insensitive(true)
                .literal_separator(true)
                .build()?;
            out.push(glob.compile_matcher());
        }
        Ok(Self(out))
    }

    pub fn allows(&self, full_name: &str) -> bool {
        self.0.iter().any(|m| m.is_match(full_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<ServerConfig, clap::Error> {
        let base = ["imulup", "--gh-webhook-secret=s", "--repo-allowlist=a/*"];
        ServerConfig::try_parse_from(base.iter().chain(args))
    }

    #[test]
    fn auth_flags() {
        assert!(parse(&["--gh-token=t", "--gh-user=u"]).is_ok());
        assert!(parse(&["--gh-app-id=1", "--gh-app-key=k"]).is_ok());
        assert!(parse(&["--gh-app-id=1", "--gh-app-key-file=/k.pem"]).is_ok());
        // Need one auth method, and only one.
        assert!(parse(&[]).is_err());
        assert!(
            parse(&[
                "--gh-token=t",
                "--gh-user=u",
                "--gh-app-id=1",
                "--gh-app-key=k"
            ])
            .is_err()
        );
        // A token needs a user; an app needs exactly one key source.
        assert!(parse(&["--gh-token=t"]).is_err());
        assert!(parse(&["--gh-app-id=1"]).is_err());
        assert!(parse(&["--gh-app-id=1", "--gh-app-key=k", "--gh-app-key-file=/k"]).is_err());
    }

    #[test]
    fn allowlist_globs() {
        let a = RepoAllowlist::new(&["acme/*".into(), "other/infra".into()]).unwrap();
        assert!(a.allows("acme/anything"));
        assert!(a.allows("ACME/Thing"));
        assert!(a.allows("other/infra"));
        assert!(!a.allows("other/app"));
        assert!(!a.allows("evil/acme"));
        let all = RepoAllowlist::new(&["*/*".into()]).unwrap();
        assert!(all.allows("x/y"));
        let empty = RepoAllowlist::new(&[]).unwrap();
        assert!(!empty.allows("x/y"));
    }
}
