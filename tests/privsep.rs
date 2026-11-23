//! Privilege separation, end to end: drop from root to a server user, check out a PR and run
//! steps as a step user, and verify the steps can't reach imulup's secrets.
//!
//! Needs Linux and root (CI runs it in a container); elsewhere it skips. It changes the whole
//! process's user, so it runs as its own binary without libtest's threads (`harness = false`).
#![allow(clippy::unwrap_used, clippy::expect_used)] // test

#[cfg(target_os = "linux")]
fn main() {
    if !imulup::privsep::is_root() {
        println!("privsep: skipped (needs root)");
        return;
    }
    linux::run();
    println!("privsep: ok");
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("privsep: skipped (Linux only)");
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Duration;

    use imulup::privsep::{self, Identity};
    use imulup::repo_config::Step;
    use imulup::runner::{StepEnv, run_steps};
    use imulup::state::{Lock, Store};
    use imulup::workspace::Workspaces;

    const SERVER: &str = "imulup-test-server";
    const STEPS: &str = "imulup-test-steps";
    const CANARY: &str = "privsep-canary-secret";

    fn sh(dir: &Path, cmd: &str) -> String {
        let out = Command::new("sh")
            .args(["-ec", cmd])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{cmd}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn ensure_user(name: &str) -> Identity {
        if Identity::lookup(name).is_err() {
            let home = format!("/home/{name}");
            let ok = Command::new("useradd")
                .args([
                    "--create-home",
                    "--home-dir",
                    &home,
                    "--shell",
                    "/bin/sh",
                    name,
                ])
                .status()
                .unwrap()
                .success();
            assert!(ok, "useradd {name}");
        }
        Identity::lookup(name).unwrap()
    }

    fn lock(repo: &str) -> Lock {
        Lock {
            repo: format!("acme/{repo}"),
            dir: ".".into(),
            stack: "dev".into(),
            project: "dev".into(),
            pull: 1,
            user: "alice".into(),
            created_at: 0,
        }
    }

    fn mode(path: &Path) -> u32 {
        path.metadata().unwrap().permissions().mode() & 0o777
    }

    pub fn run() {
        let server = ensure_user(SERVER);
        let steps_user = ensure_user(STEPS);

        // Everything lives under a world-traversable dir so both users can reach their parts.
        let base = PathBuf::from(format!("/tmp/imulup-privsep-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A root-only key file, a secret in imulup's own environment, and a PR to check out.
        let key = base.join("app-key.pem");
        std::fs::write(&key, CANARY).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o400)).unwrap();
        // SAFETY: single-threaded at this point; nothing else reads the environment.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("IMULUP_TEST_SECRET", CANARY);
        }
        let src = base.join("src");
        let remote = base.join("remote.git");
        std::fs::create_dir_all(&src).unwrap();
        sh(
            &src,
            "git init -q -b main && echo hi > README.md && git add -A \
             && git -c user.name=t -c user.email=t@example.com commit -q -m init",
        );
        let sha = sh(&src, "git rev-parse HEAD");
        sh(
            &base,
            &format!(
                "git init -q --bare {r} && git -C src push -q {r} HEAD:refs/pull/1/head",
                r = remote.display()
            ),
        );

        // Stands in for GitHub; git only serves local repos owned by the fetching user.
        sh(&base, &format!("chown -R {SERVER} {}", remote.display()));

        // A data dir with leftovers from an older, root-run version.
        let data = base.join("data");
        std::fs::create_dir_all(data.join("repos/acme/old/9")).unwrap();
        std::fs::write(data.join("repos/acme/old/9/file"), "x").unwrap();
        Store::open(&data.join("state.json"))
            .unwrap()
            .try_lock(lock("old"))
            .unwrap()
            .unwrap();
        std::fs::set_permissions(
            data.join("state.json"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        privsep::prepare_data_dir(&data, &server, Some(&steps_user)).unwrap();
        assert_eq!(mode(&data), 0o711);
        assert_eq!(mode(&data.join("state.json")), 0o600);
        assert_eq!(
            data.join("repos/acme/old/9/file").metadata().unwrap().uid(),
            steps_user.uid,
            "old checkouts are re-owned by the step user"
        );

        privsep::drop_privileges(&server, true).unwrap();
        assert!(!privsep::is_root());
        assert_eq!(
            std::fs::read(&key).unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied,
            "the key file stays root-only"
        );

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async_part(&data, &key, &remote, &sha, steps_user));

        // Cleanup needs root, which we gave up; the container is thrown away anyway.
    }

    async fn async_part(data: &Path, key: &Path, remote: &Path, sha: &str, steps_user: Identity) {
        // The server user can keep using state written by an older, root-run version.
        let store = Store::open(&data.join("state.json")).unwrap();
        store.try_lock(lock("infra")).unwrap().unwrap();
        assert_eq!(mode(&data.join("state.json")), 0o600);

        // The checkout goes through imulup's mirror and ends up owned by the step user.
        let ws = Workspaces::new(data, Some(steps_user.clone()));
        let dir = ws
            .checkout("acme/infra", 1, remote.to_str().unwrap(), sha, None)
            .await
            .unwrap();
        assert_eq!(
            dir.join("README.md").metadata().unwrap().uid(),
            steps_user.uid
        );
        let mirror = data.join("mirrors/acme/infra.git");
        assert_ne!(mirror.metadata().unwrap().uid(), steps_user.uid);
        // A second checkout of the same head reuses it.
        ws.checkout("acme/infra", 1, remote.to_str().unwrap(), sha, None)
            .await
            .unwrap();

        let pid = std::process::id();
        let probe = format!(
            r#"
echo "uid=$(id -u) home=$HOME user=$USER"
cat /proc/{pid}/environ >/dev/null 2>&1 && echo LEAK-environ || echo environ-denied
cat /proc/{pid}/mem >/dev/null 2>&1 && echo LEAK-mem || echo mem-denied
cat {key} >/dev/null 2>&1 && echo LEAK-key || echo key-denied
cat {data}/state.json >/dev/null 2>&1 && echo LEAK-state || echo state-denied
ls {data} >/dev/null 2>&1 && echo LEAK-datadir-list || echo datadir-list-denied
touch {data}/mirrors/acme/infra.git/HEAD 2>/dev/null && echo LEAK-mirror-write || echo mirror-write-denied
kill -0 {pid} 2>/dev/null && echo LEAK-signal || echo signal-denied
grep CapEff /proc/self/status
grep NoNewPrivs /proc/self/status
echo "imulup-env=${{IMULUP_TEST_SECRET:-scrubbed}}"
touch "$HOME/.probe" && echo home-writable
touch "$PWD/probe" && echo checkout-writable
"#,
            key = key.display(),
            data = data.display(),
        );
        let env = StepEnv {
            pulumi_bin: "pulumi".into(),
            stack: "dev".into(),
            project: "infra".into(),
            dir: ".".into(),
            repo: "acme/infra".into(),
            pull: 1,
            head_sha: sha.into(),
            user: "alice".into(),
            timeout: Duration::from_secs(30),
            run_as: Some(steps_user.clone()),
        };
        let r = run_steps(&[Step::Run { run: probe }], &dir, &env).await;
        let out = r.output;
        println!("{out}");
        assert!(r.success, "{out}");
        assert!(!out.contains("LEAK"), "{out}");
        assert!(
            out.contains(&format!(
                "uid={} home={} user={STEPS}",
                steps_user.uid,
                steps_user.home.display()
            )),
            "{out}"
        );
        assert!(out.contains("CapEff:\t0000000000000000"), "{out}");
        assert!(out.contains("NoNewPrivs:\t1"), "{out}");
        assert!(out.contains("imulup-env=scrubbed"), "{out}");
        assert!(
            out.contains("home-writable") && out.contains("checkout-writable"),
            "{out}"
        );

        // Steps that time out are killed even though they belong to another user.
        let pidfile = dir.join("sleep.pid");
        let env = StepEnv {
            timeout: Duration::from_secs(1),
            ..env
        };
        let sleeper = Step::Run {
            run: format!("echo $$ > {}; exec sleep 60", pidfile.display()),
        };
        let r = run_steps(&[sleeper], &dir, &env).await;
        assert!(!r.success && r.output.contains("timed out"), "{}", r.output);
        let sleep_pid = std::fs::read_to_string(&pidfile).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let status = std::fs::read_to_string(format!("/proc/{}/status", sleep_pid.trim()))
            .unwrap_or_default();
        assert!(
            status.is_empty() || status.contains("State:\tZ"),
            "timed-out step still running: {status}"
        );

        // Closing the PR removes the step user's checkout.
        ws.delete("acme/infra", 1).await.unwrap();
        assert!(!dir.exists());
    }
}
