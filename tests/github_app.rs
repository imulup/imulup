//! GitHub App authentication against a fake GitHub API: JWT signing, installation lookup,
//! scoped installation-token minting and caching, token use for API calls and git, and
//! revocation when jobs finish.
#![allow(clippy::unwrap_used)] // test harness helpers

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post, put};
use base64::Engine;
use imulup::vcs::{Credentials, GitHub, MergeMethod, Vcs};
use serde_json::{Value, json};

const APP_ID: u64 = 42;

struct Fake {
    public_key: jsonwebtoken::DecodingKey,
    mints: AtomicUsize,
    /// Request bodies of token mints.
    mint_requests: Mutex<Vec<Value>>,
    /// Tokens minted and not yet revoked.
    live: Mutex<HashSet<String>>,
    revoked: Mutex<Vec<String>>,
    comment_auth: Mutex<Vec<String>>,
    /// (token, body) of each merge request.
    merges: Mutex<Vec<(String, Value)>>,
}

type Shared = Arc<Fake>;

fn bearer(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_string()
}

/// Rejects the request unless it carries a valid app JWT for `APP_ID`.
fn check_jwt(fake: &Fake, headers: &HeaderMap) -> Result<(), StatusCode> {
    let mut v = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    v.set_issuer(&[APP_ID.to_string()]);
    v.set_required_spec_claims(&["exp", "iat", "iss"]);
    jsonwebtoken::decode::<Value>(bearer(headers), &fake.public_key, &v)
        .map(|_| ())
        .map_err(|_| StatusCode::UNAUTHORIZED)
}

async fn get_app(State(f): State<Shared>, h: HeaderMap) -> Result<Json<Value>, StatusCode> {
    check_jwt(&f, &h)?;
    Ok(Json(json!({"slug": "imulup-test"})))
}

async fn installation(
    State(f): State<Shared>,
    UrlPath((owner, repo)): UrlPath<(String, String)>,
    h: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    check_jwt(&f, &h)?;
    if (owner.as_str(), repo.as_str()) == ("acme", "infra") {
        Ok(Json(json!({"id": 7})))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

async fn access_token(
    State(f): State<Shared>,
    UrlPath(id): UrlPath<u64>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    check_jwt(&f, &h)?;
    assert_eq!(id, 7);
    f.mint_requests.lock().unwrap().push(body);
    let n = f.mints.fetch_add(1, Ordering::SeqCst) + 1;
    let token = format!("ghs_installation_{n}");
    f.live.lock().unwrap().insert(token.clone());
    Ok((
        StatusCode::CREATED,
        Json(json!({"token": token, "expires_at": "2099-01-01T00:00:00Z"})),
    ))
}

async fn revoke(State(f): State<Shared>, h: HeaderMap) -> StatusCode {
    let token = bearer(&h);
    if f.live.lock().unwrap().remove(&token) {
        f.revoked.lock().unwrap().push(token);
        StatusCode::NO_CONTENT
    } else {
        StatusCode::UNAUTHORIZED
    }
}

async fn merge(
    State(f): State<Shared>,
    UrlPath((_, _, n)): UrlPath<(String, String, u64)>,
    h: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let token = bearer(&h);
    if !f.live.lock().unwrap().contains(&token) {
        return (StatusCode::UNAUTHORIZED, Json(json!({})));
    }
    f.merges.lock().unwrap().push((token, body));
    if n == 2 {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"message": "At least 1 approving review is required"})),
        );
    }
    (StatusCode::OK, Json(json!({"merged": true})))
}

async fn comment(State(f): State<Shared>, h: HeaderMap) -> (StatusCode, Json<Value>) {
    let token = bearer(&h);
    if !f.live.lock().unwrap().contains(&token) {
        return (StatusCode::UNAUTHORIZED, Json(json!({})));
    }
    f.comment_auth.lock().unwrap().push(token);
    (StatusCode::CREATED, Json(json!({})))
}

fn openssl(dir: &Path, args: &[&str]) {
    let ok = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "openssl {args:?}");
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one token lifecycle, step by step; splitting it hides the order
async fn app_installation_tokens() {
    // Generate a throwaway key rather than committing one.
    let tmp = tempfile::tempdir().unwrap();
    openssl(tmp.path(), &["genrsa", "-out", "key.pem", "2048"]);
    openssl(
        tmp.path(),
        &["rsa", "-in", "key.pem", "-pubout", "-out", "pub.pem"],
    );
    let private_key_pem = std::fs::read_to_string(tmp.path().join("key.pem")).unwrap();
    let public_pem = std::fs::read(tmp.path().join("pub.pem")).unwrap();

    let fake = Arc::new(Fake {
        public_key: jsonwebtoken::DecodingKey::from_rsa_pem(&public_pem).unwrap(),
        mints: AtomicUsize::new(0),
        mint_requests: Mutex::default(),
        live: Mutex::default(),
        revoked: Mutex::default(),
        comment_auth: Mutex::default(),
        merges: Mutex::default(),
    });
    let router = axum::Router::new()
        .route("/app", get(get_app))
        .route("/repos/{owner}/{repo}/installation", get(installation))
        .route("/app/installations/{id}/access_tokens", post(access_token))
        .route("/installation/token", delete(revoke))
        .route("/repos/{owner}/{repo}/issues/{n}/comments", post(comment))
        .route("/repos/{owner}/{repo}/pulls/{n}/merge", put(merge))
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });

    let gh = GitHub::new(
        &api,
        Credentials::App {
            app_id: APP_ID,
            private_key_pem,
        },
    )
    .unwrap();

    assert_eq!(
        gh.app_login().await.unwrap().as_deref(),
        Some("imulup-test[bot]")
    );

    // API calls use an installation token, cached across calls (repo names are case-insensitive).
    gh.job_started("acme/infra");
    gh.comment("acme/infra", 1, "one").await.unwrap();
    gh.comment("ACME/Infra", 1, "two").await.unwrap();
    assert_eq!(fake.mints.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fake.comment_auth.lock().unwrap(),
        ["ghs_installation_1", "ghs_installation_1"]
    );

    // git gets its own read-only token as basic auth, and both forms are redacted.
    let header = gh.git_auth_header("acme/infra").await.unwrap().unwrap();
    let basic =
        base64::engine::general_purpose::STANDARD.encode("x-access-token:ghs_installation_2");
    assert_eq!(header, format!("Basic {basic}"));
    let secrets = gh.secrets();
    for s in ["ghs_installation_1", "ghs_installation_2", basic.as_str()] {
        assert!(secrets.iter().any(|x| x == s), "{s} not redacted");
    }

    // Each token covers only this repo and only what that use needs.
    assert_eq!(
        *fake.mint_requests.lock().unwrap(),
        [
            json!({"repositories": ["infra"], "permissions":
                {"pull_requests": "write", "issues": "write", "statuses": "write"}}),
            json!({"repositories": ["infra"], "permissions": {"contents": "read"}}),
        ]
    );

    // Tokens survive until the last concurrent job on the repo finishes, then are revoked.
    gh.job_started("acme/infra");
    gh.job_finished("acme/infra").await;
    assert!(fake.revoked.lock().unwrap().is_empty());
    gh.job_finished("ACME/infra").await;
    let mut revoked = fake.revoked.lock().unwrap().clone();
    revoked.sort();
    assert_eq!(revoked, ["ghs_installation_1", "ghs_installation_2"]);
    assert_eq!(gh.secrets(), Vec::<String>::new());

    // The next job mints a fresh token rather than reusing a revoked one.
    gh.job_started("acme/infra");
    gh.comment("acme/infra", 1, "three").await.unwrap();
    assert_eq!(
        fake.comment_auth.lock().unwrap().last().unwrap(),
        "ghs_installation_3"
    );

    // Merging uses its own token, the only one with write access to code, pinned to the sha.
    gh.merge("acme/infra", 1, "abc123", MergeMethod::Squash)
        .await
        .unwrap();
    assert_eq!(
        fake.mint_requests.lock().unwrap().last().unwrap(),
        &json!({"repositories": ["infra"], "permissions":
            {"contents": "write", "pull_requests": "write"}})
    );
    assert_eq!(
        *fake.merges.lock().unwrap(),
        [(
            "ghs_installation_4".to_string(),
            json!({"sha": "abc123", "merge_method": "squash"})
        )]
    );
    // GitHub's reason for refusing a merge is passed on.
    let err = gh
        .merge("acme/infra", 2, "abc123", MergeMethod::Merge)
        .await
        .unwrap_err();
    assert!(
        format!("{err:#}").contains("405 Method Not Allowed): At least 1 approving review"),
        "{err:#}"
    );
    gh.job_finished("acme/infra").await;
    assert!(
        fake.revoked
            .lock()
            .unwrap()
            .contains(&"ghs_installation_4".to_string()),
        "the merge token is revoked with the others"
    );

    // Repos the app isn't installed on fail clearly.
    let err = gh.comment("acme/other", 1, "x").await.unwrap_err();
    assert!(
        format!("{err:#}").contains("not installed on acme/other"),
        "{err:#}"
    );
}

#[test]
fn rejects_bad_app_key() {
    let err = GitHub::new(
        "http://localhost",
        Credentials::App {
            app_id: APP_ID,
            private_key_pem: "not a key".into(),
        },
    )
    .err()
    .unwrap();
    assert!(format!("{err:#}").contains("not a valid RSA PEM key"));
}
