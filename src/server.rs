//! HTTP server: GitHub webhook endpoint, health check, and a read-only lock view.

use std::fmt::Write as _;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::Value;
use sha2::Sha256;
use tracing::{debug, info};

use crate::app::{App, Event};
use crate::config::RepoAllowlist;

pub struct ServerState {
    pub app: Arc<App>,
    pub webhook_secret: String,
    pub allowlist: RepoAllowlist,
}

pub fn router(state: Arc<ServerState>) -> Router {
    Router::new()
        .route("/events", post(events))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/locks", get(locks_json))
        .route("/", get(index))
        .with_state(state)
}

pub fn verify_signature(secret: &str, body: &[u8], header: Option<&str>) -> bool {
    let Some(sig) = header.and_then(|h| h.strip_prefix("sha256=")) else {
        return false;
    };
    let Ok(sig) = hex::decode(sig) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

/// Maps a GitHub webhook to an event. `Err` carries the reason it was ignored.
pub fn parse_event(kind: &str, payload: &Value) -> Result<Event, &'static str> {
    let repo = payload["repository"]["full_name"]
        .as_str()
        .ok_or("no repository")?
        .to_string();
    let action = payload["action"].as_str().unwrap_or_default();
    match kind {
        "pull_request" => {
            let number = payload["number"].as_u64().ok_or("no PR number")?;
            match action {
                "opened" | "reopened" | "synchronize" | "ready_for_review" => {
                    Ok(Event::PullUpdated { repo, number })
                }
                "closed" => Ok(Event::PullClosed { repo, number }),
                _ => Err("pull_request action not handled"),
            }
        }
        "issue_comment" => {
            if action != "created" {
                return Err("comment action not handled");
            }
            if payload["issue"]["pull_request"].is_null() {
                return Err("comment is not on a pull request");
            }
            Ok(Event::Comment {
                repo,
                number: payload["issue"]["number"]
                    .as_u64()
                    .ok_or("no issue number")?,
                user: payload["comment"]["user"]["login"]
                    .as_str()
                    .ok_or("no comment user")?
                    .to_string(),
                body: payload["comment"]["body"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                comment_id: payload["comment"]["id"].as_u64().ok_or("no comment id")?,
            })
        }
        _ => Err("event type not handled"),
    }
}

async fn events(State(s): State<Arc<ServerState>>, headers: HeaderMap, body: Bytes) -> Response {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if !verify_signature(&s.webhook_secret, &body, header("x-hub-signature-256")) {
        return (StatusCode::UNAUTHORIZED, "bad signature").into_response();
    }
    let kind = header("x-github-event").unwrap_or_default();
    if kind == "ping" {
        return (StatusCode::OK, "pong").into_response();
    }
    let Ok(payload) = serde_json::from_slice::<Value>(&body) else {
        return (StatusCode::BAD_REQUEST, "invalid JSON").into_response();
    };
    let event = match parse_event(kind, &payload) {
        Ok(e) => e,
        Err(reason) => {
            debug!(kind, reason, "ignoring webhook");
            return (StatusCode::OK, reason).into_response();
        }
    };
    let repo = match &event {
        Event::PullUpdated { repo, .. }
        | Event::PullClosed { repo, .. }
        | Event::Comment { repo, .. } => repo,
    };
    if !s.allowlist.allows(repo) {
        info!(%repo, "ignoring event from repo not in allowlist");
        return (StatusCode::FORBIDDEN, "repo not allowlisted").into_response();
    }
    info!(?event, "accepted");
    let app = s.app.clone();
    tokio::spawn(async move { app.handle(event).await });
    (StatusCode::ACCEPTED, "accepted").into_response()
}

async fn locks_json(State(s): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(s.app.store.locks())
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn index(State(s): State<Arc<ServerState>>) -> Html<String> {
    let mut rows = String::new();
    for l in s.app.store.locks() {
        let _ = write!(
            rows,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td><a href=\"https://github.com/{}/pull/{}\">#{}</a></td><td>{}</td></tr>",
            esc(&l.repo),
            esc(&l.dir),
            esc(&l.stack),
            esc(&l.repo),
            l.pull,
            l.pull,
            esc(&l.user)
        );
    }
    let body = if rows.is_empty() {
        "<p>No locks held.</p>".to_string()
    } else {
        format!(
            "<table><tr><th>Repo</th><th>Dir</th><th>Stack</th><th>PR</th><th>User</th></tr>{rows}</table>"
        )
    };
    Html(format!(
        "<!doctype html><html><head><meta charset=utf-8><title>imulup</title><style>\
body{{font-family:system-ui,sans-serif;margin:2rem;color:#222;background:#fff}}\
table{{border-collapse:collapse}}td,th{{border:1px solid #ccc;padding:.4rem .8rem;text-align:left}}\
</style></head><body><h1>imulup locks</h1>{body}</body></html>"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sign(secret: &str, body: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn signature_verification() {
        let body = b"{\"a\":1}";
        let good = sign("s3cret", body);
        assert!(verify_signature("s3cret", body, Some(&good)));
        assert!(!verify_signature("other", body, Some(&good)));
        assert!(!verify_signature("s3cret", b"{}", Some(&good)));
        assert!(!verify_signature("s3cret", body, None));
        assert!(!verify_signature("s3cret", body, Some("sha1=abcd")));
        assert!(!verify_signature("s3cret", body, Some("sha256=zz")));
    }

    #[test]
    fn parses_events() {
        let pr = json!({"action": "synchronize", "number": 4, "repository": {"full_name": "o/r"}});
        assert!(matches!(
            parse_event("pull_request", &pr),
            Ok(Event::PullUpdated { number: 4, .. })
        ));
        let closed = json!({"action": "closed", "number": 4, "repository": {"full_name": "o/r"}});
        assert!(matches!(
            parse_event("pull_request", &closed),
            Ok(Event::PullClosed { .. })
        ));
        let labeled = json!({"action": "labeled", "number": 4, "repository": {"full_name": "o/r"}});
        assert!(parse_event("pull_request", &labeled).is_err());

        let comment = json!({
            "action": "created",
            "repository": {"full_name": "o/r"},
            "issue": {"number": 9, "pull_request": {"url": "x"}},
            "comment": {"id": 77, "body": "imulup plan", "user": {"login": "alice"}}
        });
        match parse_event("issue_comment", &comment) {
            Ok(Event::Comment {
                number,
                user,
                comment_id,
                ..
            }) => {
                assert_eq!((number, user.as_str(), comment_id), (9, "alice", 77));
            }
            other => panic!("unexpected {other:?}"),
        }
        let issue_comment = json!({
            "action": "created",
            "repository": {"full_name": "o/r"},
            "issue": {"number": 9},
            "comment": {"id": 77, "body": "imulup plan", "user": {"login": "alice"}}
        });
        assert!(parse_event("issue_comment", &issue_comment).is_err());
    }
}
