use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use imulup::app::App;
use imulup::config::ServerConfig;
use imulup::server::{ServerState, router};
use imulup::state::Store;
use imulup::vcs::GitHub;
use imulup::workspace::Workspaces;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match imulup::hardening::make_non_dumpable() {
        Ok(true) => {}
        Ok(false) => warn!(
            "can't mark the process non-dumpable on this platform; \
             steps may be able to read imulup's environment"
        ),
        Err(e) => warn!("marking the process non-dumpable failed: {e:#}"),
    }

    let mut cfg = ServerConfig::parse();
    if cfg.gh_app_key.is_some() {
        warn!(
            "IMULUP_GH_APP_KEY passes the app key in the environment; prefer IMULUP_GH_APP_KEY_FILE"
        );
    }
    let allowlist = cfg
        .repo_allowlist_matcher()
        .context("invalid --repo-allowlist")?;
    std::fs::create_dir_all(&cfg.data_dir)?;
    let store = Store::open(&cfg.data_dir.join("state.json"))?;
    let workspaces = Workspaces::new(cfg.data_dir.join("repos"));
    let vcs = Arc::new(GitHub::new(&cfg.gh_api_url, cfg.gh_credentials()?)?);
    if let Some(login) = vcs.app_login().await? {
        info!("authenticated as GitHub App {login}");
        cfg.gh_user.get_or_insert(login);
    }

    let addr = format!("{}:{}", cfg.bind, cfg.port);
    let webhook_secret = cfg.gh_webhook_secret.clone();
    let app = Arc::new(App::new(cfg, vcs, store, workspaces));
    let state = Arc::new(ServerState {
        app,
        webhook_secret,
        allowlist,
    });

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    info!("imulup listening on {addr}");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
