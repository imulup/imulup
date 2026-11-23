use std::sync::Arc;

use anyhow::{Context, Result};
use clap::Parser;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use imulup::app::App;
use imulup::config::ServerConfig;
use imulup::privsep::{self, Identity};
use imulup::server::{ServerState, router};
use imulup::state::Store;
use imulup::vcs::{Credentials, GitHub};
use imulup::workspace::Workspaces;

// Not `#[tokio::main]`: dropping privileges must happen while the process is single-threaded,
// before the runtime starts its worker threads.
fn main() -> Result<()> {
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

    let cfg = ServerConfig::parse();
    if cfg.gh_app_key.is_some() {
        warn!(
            "IMULUP_GH_APP_KEY passes the app key in the environment; prefer IMULUP_GH_APP_KEY_FILE"
        );
    }
    // Read secrets while still privileged: the key file can then be readable by root alone.
    let credentials = cfg.gh_credentials()?;
    let server_user = cfg.server_user().map(Identity::lookup).transpose()?;
    let step_user = cfg.step_user().map(Identity::lookup).transpose()?;

    if let Some(server) = &server_user {
        privsep::prepare_data_dir(&cfg.data_dir, server, step_user.as_ref())
            .context("preparing the data directory")?;
        privsep::drop_privileges(server, step_user.is_some())
            .with_context(|| format!("switching to user {}", server.name))?;
        info!("running as user {}", server.name);
    } else {
        if step_user.is_some() && !privsep::is_root() {
            anyhow::bail!("--step-user needs --server-user, or imulup running as root");
        }
        if privsep::is_root() {
            warn!("running as root; set --server-user so the webhook server doesn't");
        }
        std::fs::create_dir_all(&cfg.data_dir)?;
    }
    if let Some(u) = &step_user {
        info!("running checkouts and steps as user {}", u.name);
    } else {
        warn!(
            "steps run as imulup's own user and can read its key file and state; \
             set --step-user to separate them"
        );
    }

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(serve(cfg, credentials, step_user))
}

async fn serve(
    mut cfg: ServerConfig,
    credentials: Credentials,
    step_user: Option<Identity>,
) -> Result<()> {
    let allowlist = cfg
        .repo_allowlist_matcher()
        .context("invalid --repo-allowlist")?;
    let store = Store::open(&cfg.data_dir.join("state.json"))?;
    let workspaces = Workspaces::new(&cfg.data_dir, step_user);
    let vcs = Arc::new(GitHub::new(&cfg.gh_api_url, credentials)?);
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
