//! End-to-end flow against a fake GitHub, a real local git remote, and a fake `pulumi` binary.
#![allow(clippy::unwrap_used)] // test harness helpers

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use imulup::app::{App, Event};
use imulup::config::ServerConfig;
use imulup::state::Store;
use imulup::vcs::{PullRequest, StatusState, Vcs};
use imulup::workspace::Workspaces;

#[derive(Default)]
struct FakeState {
    pulls: Vec<PullRequest>,
    modified: Vec<String>,
    comments: Vec<(u64, String)>,
    statuses: Vec<(String, StatusState, String)>,
    approved: bool,
    writers: Vec<String>,
}

#[derive(Default, Clone)]
struct FakeGitHub(Arc<Mutex<FakeState>>);

impl FakeGitHub {
    fn last_comment(&self) -> String {
        self.0
            .lock()
            .unwrap()
            .comments
            .last()
            .map(|c| c.1.clone())
            .unwrap_or_default()
    }
    fn comment_count(&self) -> usize {
        self.0.lock().unwrap().comments.len()
    }
    fn last_status(&self, ctx: &str) -> Option<StatusState> {
        self.0
            .lock()
            .unwrap()
            .statuses
            .iter()
            .rev()
            .find(|s| s.0 == ctx)
            .map(|s| s.1)
    }
}

#[async_trait]
impl Vcs for FakeGitHub {
    async fn get_pull(&self, _repo: &str, number: u64) -> Result<PullRequest> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .pulls
            .iter()
            .find(|p| p.number == number)
            .cloned()
            .expect("pull"))
    }
    async fn modified_files(&self, _: &str, _: u64) -> Result<Vec<String>> {
        Ok(self.0.lock().unwrap().modified.clone())
    }
    async fn comment(&self, _: &str, number: u64, body: &str) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .comments
            .push((number, body.to_string()));
        Ok(())
    }
    async fn react(&self, _: &str, _: u64, _: &str) -> Result<()> {
        Ok(())
    }
    async fn set_status(
        &self,
        _: &str,
        _: &str,
        state: StatusState,
        ctx: &str,
        desc: &str,
    ) -> Result<()> {
        self.0
            .lock()
            .unwrap()
            .statuses
            .push((ctx.into(), state, desc.into()));
        Ok(())
    }
    async fn is_approved(&self, _: &str, _: u64, _: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().approved)
    }
    async fn has_write_access(&self, _: &str, user: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().writers.iter().any(|w| w == user))
    }
    fn git_auth_header(&self) -> Option<String> {
        None
    }
    fn secrets(&self) -> Vec<String> {
        vec!["super-secret-token".into()]
    }
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: PathBuf, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Creates a source repo, a bare "GitHub" remote, and pushes `refs/pull/<n>/head`. Returns head sha.
fn push_pr(src: &Path, bare: &Path, pull: u64) -> String {
    let sha = sh(src, &["rev-parse", "HEAD"]);
    sh(
        src,
        &[
            "push",
            "-q",
            "--force",
            bare.to_str().unwrap(),
            &format!("HEAD:refs/pull/{pull}/head"),
        ],
    );
    sha
}

fn fake_pulumi(dir: &Path) -> PathBuf {
    let path = dir.join("pulumi");
    write(
        path.clone(),
        r#"#!/bin/sh
# Fake pulumi: logs invocations, fails for stack "broken", leaks the token to test redaction.
echo "$@" >> "$(dirname "$0")/pulumi.log"
stack=""; prev=""
for a in "$@"; do [ "$prev" = "--stack" ] && stack="$a"; prev="$a"; done
[ "$stack" = "broken" ] && { echo "error: stack is broken" >&2; exit 1; }
echo "cwd=$(basename "$PWD") token=super-secret-token gh=${IMULUP_GH_TOKEN:-unset}"
case "$1" in
  preview) printf 'Previewing update (%s)\n    + aws:s3:Bucket site create\nResources:\n    + 1 to create\n    2 unchanged\n' "$stack" ;;
  up) printf 'Updating (%s)\n    + aws:s3:Bucket site created\nResources:\n    + 1 created\n' "$stack" ;;
esac
"#,
    );
    Command::new("chmod")
        .args(["+x", path.to_str().unwrap()])
        .status()
        .unwrap();
    path
}

struct Harness {
    _tmp: tempfile::TempDir,
    src: PathBuf,
    bare: PathBuf,
    log: PathBuf,
    gh: FakeGitHub,
    app: App,
}

impl Harness {
    fn new(extra_args: &[&str]) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let src = root.join("src");
        let bare = root.join("remote.git");
        std::fs::create_dir_all(&src).unwrap();
        sh(&src, &["init", "-q", "-b", "main"]);
        write(
            src.join("infra/Pulumi.yaml"),
            "name: infra\nruntime: nodejs\n",
        );
        write(src.join("infra/Pulumi.dev.yaml"), "config: {}\n");
        write(src.join("infra/Pulumi.prod.yaml"), "config: {}\n");
        write(src.join("infra/index.ts"), "export const x = 1;\n");
        write(src.join("README.md"), "hi\n");
        sh(&src, &["add", "-A"]);
        sh(&src, &["commit", "-q", "-m", "init"]);
        sh(&root, &["init", "-q", "--bare", bare.to_str().unwrap()]);

        let pulumi = fake_pulumi(&root);
        let log = root.join("pulumi.log");
        // SAFETY: every test sets the same value; the runner must scrub it from child processes.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("IMULUP_GH_TOKEN", "should-not-leak");
        };

        let data = root.join("data");
        let mut args = vec![
            "imulup",
            "--gh-token=x",
            "--gh-webhook-secret=y",
            "--gh-user=imulup-bot",
            "--repo-allowlist=o/*",
        ];
        let pulumi_arg = format!("--pulumi-bin={}", pulumi.display());
        args.push(&pulumi_arg);
        args.extend(extra_args);
        let cfg = ServerConfig::parse_from(args);
        let gh = FakeGitHub::default();
        gh.0.lock().unwrap().writers = vec!["alice".into()];
        let app = App::new(
            cfg,
            Arc::new(gh.clone()),
            Store::open(&data.join("state.json")).unwrap(),
            Workspaces::new(data.join("repos")),
        );
        Harness {
            _tmp: tmp,
            src,
            bare,
            log,
            gh,
            app,
        }
    }

    fn open_pr(&self, number: u64, modified: &[&str]) -> String {
        let sha = push_pr(&self.src, &self.bare, number);
        let mut s = self.gh.0.lock().unwrap();
        s.pulls.retain(|p| p.number != number);
        s.pulls.push(PullRequest {
            repo: "o/r".into(),
            number,
            head_sha: sha.clone(),
            author: "bob".into(),
            open: true,
            draft: false,
            is_fork: false,
            mergeable: Some(true),
            clone_url: self.bare.to_str().unwrap().into(),
        });
        s.modified = modified
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        sha
    }

    async fn comment(&self, number: u64, user: &str, body: &str) -> String {
        self.app
            .handle(Event::Comment {
                repo: "o/r".into(),
                number,
                user: user.into(),
                body: body.into(),
                comment_id: 1,
            })
            .await;
        self.gh.last_comment()
    }

    fn pulumi_calls(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

#[tokio::test]
async fn full_pr_lifecycle() {
    let h = Harness::new(&["--require-approval"]);

    // Autoplan on open: both stacks of the touched project get previewed.
    h.open_pr(1, &["infra/index.ts"]);
    h.app
        .handle(Event::PullUpdated {
            repo: "o/r".into(),
            number: 1,
        })
        .await;
    let c = h.gh.last_comment();
    assert!(c.contains("imulup plan — 2/2 projects succeeded"), "{c}");
    assert!(c.contains("`infra:dev` — changes pending"), "{c}");
    assert!(c.contains("`imulup apply -d infra -s prod`"), "{c}");
    assert!(
        c.contains("cwd=infra"),
        "pulumi must run in the project dir: {c}"
    );
    assert!(c.contains("gh=unset"), "imulup env must be scrubbed: {c}");
    assert!(
        !c.contains("super-secret-token"),
        "secrets must be redacted: {c}"
    );
    assert_eq!(h.gh.last_status("imulup/plan"), Some(StatusState::Success));
    assert_eq!(h.gh.last_status("imulup/apply"), Some(StatusState::Pending));
    assert_eq!(h.app.store.locks().len(), 2);
    assert!(h.pulumi_calls()[0].starts_with("preview --stack dev --non-interactive"));

    // A PR touching unrelated files doesn't plan anything.
    let before = h.gh.comment_count();
    h.open_pr(5, &["README.md"]);
    h.app
        .handle(Event::PullUpdated {
            repo: "o/r".into(),
            number: 5,
        })
        .await;
    assert_eq!(h.gh.comment_count(), before);

    // Another PR can't plan the locked stacks.
    h.open_pr(2, &["infra/index.ts"]);
    let c = h.comment(2, "alice", "imulup plan -d infra -s dev").await;
    assert!(c.contains("locked by #1"), "{c}");
    assert_eq!(h.gh.last_status("imulup/plan"), Some(StatusState::Failure));

    // Non-writers are refused; unknown commands get usage.
    let c = h.comment(1, "mallory", "imulup apply").await;
    assert!(c.contains("needs write access"), "{c}");
    let c = h.comment(1, "alice", "imulup destroy").await;
    assert!(
        c.contains("unknown command") && c.contains("Commands:"),
        "{c}"
    );
    // The bot's own comments are ignored.
    let before = h.gh.comment_count();
    h.comment(1, "imulup-bot", "imulup plan").await;
    assert_eq!(h.gh.comment_count(), before);

    // Apply is blocked until approved.
    let c = h.comment(1, "alice", "imulup apply").await;
    assert!(c.contains("requirement `approved` is not met"), "{c}");
    h.gh.0.lock().unwrap().approved = true;

    // Apply a single stack, then the rest.
    let c = h.comment(1, "alice", "imulup apply -s dev").await;
    assert!(c.contains("imulup apply — 1/1 project succeeded"), "{c}");
    assert!(c.contains("1 created"), "{c}");
    assert!(
        h.pulumi_calls()
            .iter()
            .any(|l| l.starts_with("up --stack dev") && l.contains("--yes"))
    );
    assert_eq!(h.gh.last_status("imulup/apply"), Some(StatusState::Pending));
    let c = h.comment(1, "alice", "imulup apply").await;
    assert!(
        c.contains("1/1 project succeeded") && c.contains("infra:prod"),
        "{c}"
    );
    assert_eq!(h.gh.last_status("imulup/apply"), Some(StatusState::Success));
    let c = h.comment(1, "alice", "imulup apply -s dev").await;
    assert!(c.contains("already applied"), "{c}");

    // A new push makes the plan stale.
    write(h.src.join("infra/index.ts"), "export const x = 2;\n");
    sh(&h.src, &["commit", "-q", "-am", "change"]);
    h.open_pr(1, &["infra/index.ts"]);
    let c = h.comment(1, "alice", "imulup apply -s dev").await;
    assert!(c.contains("plan is for commit"), "{c}");

    // Closing PR 1 releases its locks and workspace, so PR 2 can proceed.
    h.app
        .handle(Event::PullClosed {
            repo: "o/r".into(),
            number: 1,
        })
        .await;
    assert!(h.gh.last_comment().contains("released these locks"));
    assert!(!h.app.workspaces.dir("o/r", 1).exists());
    let c = h.comment(2, "alice", "imulup plan").await;
    assert!(c.contains("2/2 projects succeeded"), "{c}");

    // Unlock discards plans.
    let c = h.comment(2, "alice", "imulup unlock").await;
    assert!(c.contains("Released locks"), "{c}");
    assert_eq!(h.app.store.locks(), [] as [imulup::state::Lock; 0]);
    let c = h.comment(2, "alice", "imulup apply").await;
    assert!(c.contains("nothing to apply"), "{c}");
}

#[tokio::test]
async fn repo_config_workflows_and_failures() {
    let h = Harness::new(&[]);
    write(
        h.src.join("imulup.yaml"),
        r#"
version: 1
projects:
  - name: infra-dev
    dir: infra
    stack: dev
    workflow: custom
    autoplan:
      when_modified: ["*.ts"]
  - name: infra-broken
    dir: infra
    stack: broken
    apply_requirements: [mergeable]
  - name: manual
    dir: infra
    stack: prod
    autoplan:
      enabled: false
workflows:
  custom:
    plan:
      steps:
        - run: echo "custom step for $IMULUP_STACK in $IMULUP_DIR"
        - preview:
            extra_args: ["--refresh"]
"#,
    );
    write(h.src.join("infra/Pulumi.broken.yaml"), "config: {}\n");
    sh(&h.src, &["add", "-A"]);
    sh(&h.src, &["commit", "-q", "-m", "config"]);
    h.open_pr(3, &["infra/index.ts"]);

    h.app
        .handle(Event::PullUpdated {
            repo: "o/r".into(),
            number: 3,
        })
        .await;
    let c = h.gh.last_comment();
    assert!(c.contains("1/2 projects succeeded"), "{c}");
    assert!(c.contains("custom step for dev in infra"), "{c}");
    assert!(
        c.contains("❌ `infra-broken` — failed") && c.contains("stack is broken"),
        "{c}"
    );
    assert!(
        !c.contains("`manual`"),
        "autoplan disabled project must not plan: {c}"
    );
    assert!(
        h.pulumi_calls()
            .iter()
            .any(|l| l.starts_with("preview --stack dev") && l.ends_with("--refresh"))
    );
    assert_eq!(h.gh.last_status("imulup/plan"), Some(StatusState::Failure));

    // Explicit plan by name works for autoplan-disabled projects; failed plans can't be applied.
    let c = h.comment(3, "alice", "imulup plan -p manual").await;
    assert!(
        c.contains("1/1 project succeeded") && c.contains("imulup apply -p manual"),
        "{c}"
    );
    let c = h.comment(3, "alice", "imulup apply -p infra-broken").await;
    assert!(c.contains("last plan failed"), "{c}");
    let c = h.comment(3, "alice", "imulup plan -p nope").await;
    assert!(c.contains("no project named `nope`"), "{c}");

    // `apply` with no args applies every successful plan.
    let c = h.comment(3, "alice", "imulup apply").await;
    assert!(c.contains("2/2 projects succeeded"), "{c}");

    // Invalid config is reported on the PR.
    write(
        h.src.join("imulup.yaml"),
        "version: 1\nprojects: [{dir: infra, stack: dev, oops: 1}]\n",
    );
    sh(&h.src, &["commit", "-q", "-am", "bad config"]);
    h.open_pr(3, &["infra/index.ts"]);
    h.app
        .handle(Event::PullUpdated {
            repo: "o/r".into(),
            number: 3,
        })
        .await;
    let c = h.gh.last_comment();
    assert!(c.contains("invalid imulup.yaml"), "{c}");
}
