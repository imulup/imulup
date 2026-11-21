//! Parsing of PR comment commands like `imulup plan -d infra -s dev`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    Plan,
    Apply,
    Unlock,
}

impl CommandKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CommandKind::Plan => "plan",
            CommandKind::Apply => "apply",
            CommandKind::Unlock => "unlock",
        }
    }
}

/// Narrows a command to specific projects. All `None` means "every relevant project".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFilter {
    pub project: Option<String>,
    pub dir: Option<String>,
    pub stack: Option<String>,
}

impl ProjectFilter {
    pub fn is_empty(&self) -> bool {
        self.project.is_none() && self.dir.is_none() && self.stack.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Comment isn't addressed to us.
    NotForUs,
    Help,
    /// Addressed to us but malformed; the message explains why.
    Error(String),
    Command {
        kind: CommandKind,
        filter: ProjectFilter,
    },
}

pub fn usage(name: &str) -> String {
    format!(
        "```
{name} <command> [options]

Commands:
  plan     Run `pulumi preview` for projects changed in this PR (or those selected)
  apply    Run `pulumi up` for planned projects (or those selected)
  unlock   Release this PR's locks and discard its plans
  help     Show this help

Options (plan/apply):
  -p, --project <name>   Project name from imulup.yaml
  -d, --dir <dir>        Project directory, relative to the repo root
  -s, --stack <stack>    Pulumi stack
```"
    )
}

pub fn parse(comment: &str, name: &str) -> Parsed {
    // Only the first non-empty line is considered, so commands can be followed by prose.
    let Some(line) = comment.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return Parsed::NotForUs;
    };
    let mut words = line.split_whitespace();
    if words.next() != Some(name) {
        return Parsed::NotForUs;
    }
    let args = match shell_words::split(line) {
        Ok(a) => a,
        Err(e) => return Parsed::Error(format!("could not parse command: {e}")),
    };
    let mut args = args.into_iter().skip(1);
    let kind = match args.next().as_deref() {
        None | Some("help" | "--help" | "-h") => return Parsed::Help,
        Some("plan") => CommandKind::Plan,
        Some("apply") => CommandKind::Apply,
        Some("unlock") => CommandKind::Unlock,
        Some(other) => return Parsed::Error(format!("unknown command `{other}`")),
    };

    let mut filter = ProjectFilter::default();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let slot = match flag.as_str() {
            "-h" | "--help" => return Parsed::Help,
            "-p" | "--project" => &mut filter.project,
            "-d" | "--dir" => &mut filter.dir,
            "-s" | "--stack" => &mut filter.stack,
            other => return Parsed::Error(format!("unknown option `{other}`")),
        };
        if kind == CommandKind::Unlock {
            return Parsed::Error(
                "`unlock` takes no options; it releases all of this PR's locks".into(),
            );
        }
        let value = match inline.or_else(|| args.next()) {
            Some(v) if !v.is_empty() && !v.starts_with('-') => v,
            _ => return Parsed::Error(format!("option `{flag}` needs a value")),
        };
        if slot.replace(value).is_some() {
            return Parsed::Error(format!("option `{flag}` given more than once"));
        }
    }
    if filter.project.is_some() && (filter.dir.is_some() || filter.stack.is_some()) {
        return Parsed::Error("use either --project or --dir/--stack, not both".into());
    }
    if let Some(d) = &filter.dir
        && (d.split('/').any(|seg| seg == "..") || d.starts_with('/'))
    {
        return Parsed::Error("--dir must be a relative path inside the repo".into());
    }
    Parsed::Command { kind, filter }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(kind: CommandKind, p: Option<&str>, d: Option<&str>, s: Option<&str>) -> Parsed {
        Parsed::Command {
            kind,
            filter: ProjectFilter {
                project: p.map(Into::into),
                dir: d.map(Into::into),
                stack: s.map(Into::into),
            },
        }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(
            parse("imulup plan", "imulup"),
            cmd(CommandKind::Plan, None, None, None)
        );
        assert_eq!(
            parse("  imulup apply -d infra --stack=prod\nthanks!", "imulup"),
            cmd(CommandKind::Apply, None, Some("infra"), Some("prod"))
        );
        assert_eq!(
            parse("imulup plan -p network", "imulup"),
            cmd(CommandKind::Plan, Some("network"), None, None)
        );
        assert_eq!(
            parse("imulup unlock", "imulup"),
            cmd(CommandKind::Unlock, None, None, None)
        );
        assert_eq!(parse("imulup", "imulup"), Parsed::Help);
        assert_eq!(parse("imulup help", "imulup"), Parsed::Help);
    }

    #[test]
    fn ignores_other_comments() {
        assert_eq!(parse("LGTM", "imulup"), Parsed::NotForUs);
        assert_eq!(parse("please run imulup plan", "imulup"), Parsed::NotForUs);
        assert_eq!(parse("imulupplan", "imulup"), Parsed::NotForUs);
        assert_eq!(parse("", "imulup"), Parsed::NotForUs);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(matches!(
            parse("imulup destroy", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan -x", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan -d", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan -d a -d b", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan -p a -s b", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan -d ../etc", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup unlock -s dev", "imulup"),
            Parsed::Error(_)
        ));
        assert!(matches!(
            parse("imulup plan \"unterminated", "imulup"),
            Parsed::Error(_)
        ));
    }
}
