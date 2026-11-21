//! Markdown rendering for PR comments.

use std::fmt::Write as _;

use crate::command::CommandKind;

/// GitHub rejects comments over 65536 chars; leave headroom for the surrounding markup.
pub const MAX_COMMENT: usize = 60_000;

#[derive(Debug, Clone)]
pub enum Outcome {
    Success {
        output: String,
        has_changes: Option<bool>,
    },
    Failure {
        output: String,
    },
    /// Didn't run (lock held, plan stale, requirement unmet…).
    Skipped {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct ProjectResult {
    pub project: String,
    pub selector: String,
    pub outcome: Outcome,
}

impl ProjectResult {
    pub fn ok(&self) -> bool {
        matches!(self.outcome, Outcome::Success { .. })
    }
}

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut out = text.to_string();
    for s in secrets.iter().filter(|s| s.len() >= 6) {
        out = out.replace(s.as_str(), "***");
    }
    out
}

/// Keep the tail of long output: pulumi's summary and errors are at the end.
fn tail(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_string();
    }
    let mut start = text.len() - budget;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let cut = text[start..].find('\n').map_or(start, |i| start + i + 1);
    format!("… {} bytes truncated …\n{}", cut, &text[cut..])
}

/// Move pulumi's indented `+`/`-`/`~` markers to column 0 so GitHub's diff highlighting applies.
fn diffify(text: &str) -> String {
    text.lines()
        .map(|l| {
            let trimmed = l.trim_start();
            let indent = &l[..l.len() - trimmed.len()];
            for (marker, rep) in [
                ("+ ", "+"),
                ("- ", "-"),
                ("~ ", "!"),
                ("+-", "!"),
                ("-+", "!"),
            ] {
                if trimmed.starts_with(marker) {
                    return format!("{rep}{indent}{}", &trimmed[marker.trim_end().len()..]);
                }
            }
            format!(" {l}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fence(text: &str, lang: &str) -> String {
    // Neutralize backtick fences inside output so they can't break out of the code block.
    let safe = text.trim_end().replace("```", "`\u{200b}``");
    format!("```{lang}\n{safe}\n```")
}

pub fn results(
    kind: CommandKind,
    results: &[ProjectResult],
    command_name: &str,
    secrets: &[String],
) -> String {
    let n_ok = results.iter().filter(|r| r.ok()).count();
    let mut out = format!(
        "## imulup {} — {}/{} project{} succeeded\n\n",
        kind.as_str(),
        n_ok,
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );
    let budget = MAX_COMMENT.saturating_sub(4_000) / results.len().max(1);
    for r in results {
        let (icon, note, body) = match &r.outcome {
            Outcome::Success {
                output,
                has_changes,
            } => {
                let note = match (kind, has_changes) {
                    (CommandKind::Plan, Some(false)) => " — no changes",
                    (CommandKind::Plan, Some(true)) => " — changes pending",
                    _ => "",
                };
                ("✅", note, Some(output))
            }
            Outcome::Failure { output } => ("❌", " — failed", Some(output)),
            Outcome::Skipped { .. } => ("⏸️", " — skipped", None),
        };
        let _ = write!(out, "### {icon} `{}`{note}\n\n", r.project);
        if let Outcome::Skipped { reason } = &r.outcome {
            let _ = write!(out, "{}\n\n", redact(reason, secrets));
        }
        if let Some(body) = body {
            let text = tail(&redact(body, secrets), budget);
            let lang = if kind == CommandKind::Plan {
                "diff"
            } else {
                ""
            };
            let text = if kind == CommandKind::Plan {
                diffify(&text)
            } else {
                text
            };
            let open = if r.ok() { "" } else { " open" };
            let _ = write!(
                out,
                "<details{open}><summary>Output</summary>\n\n{}\n\n</details>\n\n",
                fence(&text, lang)
            );
        }
        if kind == CommandKind::Plan && r.ok() {
            let _ = writeln!(out, "* To apply: `{command_name} apply {}`", r.selector);
            let _ = write!(
                out,
                "* To re-plan: `{command_name} plan {}`\n\n",
                r.selector
            );
        }
    }
    if kind == CommandKind::Plan && n_ok > 0 {
        let _ = write!(
            out,
            "---\n* Apply all planned projects: `{command_name} apply`\n* Discard plans and release locks: `{command_name} unlock`\n"
        );
    }
    out
}

pub fn error(kind: Option<CommandKind>, msg: &str, secrets: &[String]) -> String {
    let what = kind.map(|k| format!(" {}", k.as_str())).unwrap_or_default();
    format!(
        "## ❌ imulup{what} error\n\n{}",
        fence(&redact(msg, secrets), "")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pr(outcome: Outcome) -> ProjectResult {
        ProjectResult {
            project: "infra:dev".into(),
            selector: "-d infra -s dev".into(),
            outcome,
        }
    }

    #[test]
    fn renders_plan() {
        let out = results(
            CommandKind::Plan,
            &[pr(Outcome::Success {
                output: "    + aws:s3:Bucket b create\n```evil\ntoken ghp_abcdef123\n".into(),
                has_changes: Some(true),
            })],
            "imulup",
            &["ghp_abcdef123".into()],
        );
        assert!(out.contains("1/1 project succeeded"));
        assert!(out.contains("changes pending"));
        assert!(out.contains("\n+     aws:s3:Bucket b create"));
        assert!(out.contains("`imulup apply -d infra -s dev`"));
        assert!(!out.contains("ghp_abcdef123"));
        assert_eq!(
            out.matches("```").count(),
            2,
            "inner fence must be neutralized:\n{out}"
        );
    }

    #[test]
    fn truncates_long_output() {
        let long = "line of output\n".repeat(20_000);
        let out = results(
            CommandKind::Apply,
            &[pr(Outcome::Failure { output: long })],
            "imulup",
            &[],
        );
        assert!(out.len() < MAX_COMMENT);
        assert!(out.contains("bytes truncated"));
        assert!(out.contains("<details open>"));
    }

    #[test]
    fn renders_skip() {
        let out = results(
            CommandKind::Apply,
            &[pr(Outcome::Skipped {
                reason: "locked by #3".into(),
            })],
            "imulup",
            &[],
        );
        assert!(out.contains("0/1 projects succeeded") || out.contains("0/1 project succeeded"));
        assert!(out.contains("locked by #3"));
    }
}
