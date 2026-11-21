//! Executes workflow steps (pulumi preview/up and custom `run:` commands).

use std::fmt::Write as _;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::repo_config::Step;

/// Context exposed to steps; `run:` steps also see these as `IMULUP_*` env vars.
#[derive(Debug, Clone)]
pub struct StepEnv {
    pub pulumi_bin: String,
    pub stack: String,
    pub project: String,
    pub dir: String,
    pub repo: String,
    pub pull: u64,
    pub head_sha: String,
    pub user: String,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct StepResult {
    pub output: String,
    pub success: bool,
}

/// Runs steps in order, stopping at the first failure. Output of all steps is concatenated.
pub async fn run_steps(steps: &[Step], project_dir: &Path, env: &StepEnv) -> StepResult {
    let mut output = String::new();
    for step in steps {
        let (label, argv) = command_for(step, env);
        if steps.len() > 1 {
            let _ = writeln!(output, "$ {label}");
        }
        let r = run(&argv, project_dir, env).await;
        output.push_str(&r.output);
        if !output.ends_with('\n') && !output.is_empty() {
            output.push('\n');
        }
        if !r.success {
            return StepResult {
                output,
                success: false,
            };
        }
    }
    StepResult {
        output,
        success: true,
    }
}

fn pulumi_args(env: &StepEnv, verb: &str, extra: &[String]) -> Vec<String> {
    let mut a = vec![
        env.pulumi_bin.clone(),
        verb.to_string(),
        "--stack".into(),
        env.stack.clone(),
    ];
    a.extend(["--non-interactive", "--diff", "--color", "never"].map(String::from));
    if verb == "up" {
        a.extend(["--yes", "--skip-preview"].map(String::from));
    }
    a.extend(extra.iter().cloned());
    a
}

fn command_for(step: &Step, env: &StepEnv) -> (String, Vec<String>) {
    let argv = match step {
        Step::Builtin(s) if s == "up" => pulumi_args(env, "up", &[]),
        Step::Builtin(_) => pulumi_args(env, "preview", &[]),
        Step::Preview { preview } => pulumi_args(env, "preview", &preview.extra_args),
        Step::Up { up } => pulumi_args(env, "up", &up.extra_args),
        Step::Run { run } => vec!["sh".into(), "-c".into(), run.clone()],
    };
    let label = match step {
        Step::Run { run } => run.clone(),
        _ => argv.join(" "),
    };
    (label, argv)
}

async fn run(argv: &[String], cwd: &Path, env: &StepEnv) -> StepResult {
    // `sh -c 'exec "$@" 2>&1'` interleaves stdout and stderr in one pipe, in order.
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg("exec \"$@\" 2>&1").arg("sh").args(argv);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    // Never leak imulup's own config (GitHub token, webhook secret) into user code.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("IMULUP_") {
            cmd.env_remove(k);
        }
    }
    cmd.env("PULUMI_SKIP_UPDATE_CHECK", "true")
        .env("CI", "true")
        .env("IMULUP_STACK", &env.stack)
        .env("IMULUP_PROJECT", &env.project)
        .env("IMULUP_DIR", &env.dir)
        .env("IMULUP_REPO", &env.repo)
        .env("IMULUP_PULL", env.pull.to_string())
        .env("IMULUP_HEAD_SHA", &env.head_sha)
        .env("IMULUP_USER", &env.user);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                output: format!("failed to start {}: {e}", argv[0]),
                success: false,
            };
        }
    };
    match tokio::time::timeout(env.timeout, child.wait_with_output()).await {
        Err(_) => StepResult {
            output: format!("step timed out after {}s", env.timeout.as_secs()),
            success: false,
        },
        Ok(Err(e)) => StepResult {
            output: format!("failed waiting for {}: {e}", argv[0]),
            success: false,
        },
        Ok(Ok(out)) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            StepResult {
                output: strip_ansi(&text),
                success: out.status.success(),
            }
        }
    }
}

/// Best-effort detection of pending changes from `pulumi preview` output.
pub fn preview_has_changes(output: &str) -> Option<bool> {
    let idx = output.rfind("Resources:")?;
    let summary = &output[idx..];
    let changes = [
        "to create",
        "to update",
        "to delete",
        "to replace",
        "to import",
    ];
    Some(
        summary
            .lines()
            .take(12)
            .any(|l| changes.iter().any(|c| l.contains(c))),
    )
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if ('@'..='~').contains(&c2) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_changes() {
        let changed =
            "Previewing update (dev)\n...\nResources:\n    + 1 to create\n    3 unchanged\n";
        let none = "Previewing update (dev)\nResources:\n    4 unchanged\n";
        assert_eq!(preview_has_changes(changed), Some(true));
        assert_eq!(preview_has_changes(none), Some(false));
        assert_eq!(preview_has_changes("error: boom"), None);
    }

    #[test]
    fn strips_ansi() {
        assert_eq!(strip_ansi("\u{1b}[32m+ create\u{1b}[0m ok"), "+ create ok");
    }

    #[tokio::test]
    async fn runs_steps_and_scrubs_env() {
        let tmp = tempfile::tempdir().unwrap();
        let env = StepEnv {
            pulumi_bin: "pulumi".into(),
            stack: "dev".into(),
            project: "p".into(),
            dir: ".".into(),
            repo: "o/r".into(),
            pull: 7,
            head_sha: "abc".into(),
            user: "u".into(),
            timeout: Duration::from_secs(10),
        };
        // SAFETY: test-only; no other threads read this var.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("IMULUP_GH_TOKEN", "sekrit");
        };
        let steps = vec![
            Step::Run {
                run:
                    "echo out; echo err >&2; echo token=${IMULUP_GH_TOKEN:-none} pull=$IMULUP_PULL"
                        .into(),
            },
            Step::Run {
                run: "exit 3".into(),
            },
            Step::Run {
                run: "echo never".into(),
            },
        ];
        let r = run_steps(&steps, tmp.path(), &env).await;
        assert!(!r.success);
        assert!(r.output.contains("out\n"));
        assert!(r.output.contains("err\n"));
        assert!(r.output.contains("token=none pull=7"));
        assert!(!r.output.contains("never"));

        let slow = StepEnv {
            timeout: Duration::from_millis(200),
            ..env
        };
        let r = run_steps(
            &[Step::Run {
                run: "sleep 5".into(),
            }],
            tmp.path(),
            &slow,
        )
        .await;
        assert!(!r.success && r.output.contains("timed out"));
    }
}
