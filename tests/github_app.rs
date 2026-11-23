//! GitHub App authentication against a fake GitHub API: JWT signing, installation lookup,
//! installation-token minting and caching, and token use for API calls and git.
#![allow(clippy::unwrap_used)] // test harness helpers

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use base64::Engine;
use imulup::vcs::{Credentials, GitHub, Vcs};
use serde_json::{Value, json};

const APP_ID: u64 = 42;

struct Fake {
    public_key: jsonwebtoken::DecodingKey,
    mints: AtomicUsize,
    comment_auth: Mutex<Vec<String>>,
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
) -> Result<(StatusCode, Json<Value>), StatusCode> {
    check_jwt(&f, &h)?;
    assert_eq!(id, 7);
    let n = f.mints.fetch_add(1, Ordering::SeqCst) + 1;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"token": format!("ghs_installation_{n}"), "expires_at": "2099-01-01T00:00:00Z"}),
        ),
    ))
}

async fn comment(State(f): State<Shared>, h: HeaderMap) -> (StatusCode, Json<Value>) {
    f.comment_auth.lock().unwrap().push(bearer(&h));
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
        comment_auth: Mutex::default(),
    });
    let router = axum::Router::new()
        .route("/app", get(get_app))
        .route("/repos/{owner}/{repo}/installation", get(installation))
        .route("/app/installations/{id}/access_tokens", post(access_token))
        .route("/repos/{owner}/{repo}/issues/{n}/comments", post(comment))
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

    // API calls use the installation token, and it is cached across calls.
    gh.comment("acme/infra", 1, "one").await.unwrap();
    gh.comment("ACME/Infra", 1, "two").await.unwrap();
    assert_eq!(fake.mints.load(Ordering::SeqCst), 1);
    assert_eq!(
        *fake.comment_auth.lock().unwrap(),
        ["ghs_installation_1", "ghs_installation_1"]
    );

    // git gets the same token as basic auth, and both forms are redacted.
    let header = gh.git_auth_header("acme/infra").await.unwrap().unwrap();
    let basic =
        base64::engine::general_purpose::STANDARD.encode("x-access-token:ghs_installation_1");
    assert_eq!(header, format!("Basic {basic}"));
    let secrets = gh.secrets();
    assert!(secrets.contains(&"ghs_installation_1".to_string()));
    assert!(secrets.contains(&basic));

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
