//! `imulup.yaml` parsing and discovery of which Pulumi projects/stacks a PR touches.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;

pub const CONFIG_FILES: [&str; 2] = ["imulup.yaml", "imulup.yml"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Requirement {
    /// At least one approving review from someone other than the PR author.
    Approved,
    /// GitHub reports the PR as mergeable (no conflicts).
    Mergeable,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Requirement::Approved => "approved",
            Requirement::Mergeable => "mergeable",
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StepArgs {
    #[serde(default)]
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Step {
    /// `- preview` or `- up`
    Builtin(String),
    /// `- preview: {extra_args: [...]}`
    Preview { preview: StepArgs },
    /// `- up: {extra_args: [...]}`
    Up { up: StepArgs },
    /// `- run: npm ci` — a shell command run in the project directory.
    Run { run: String },
}

impl Step {
    fn validate(&self) -> Result<()> {
        match self {
            Step::Builtin(s) if s == "preview" || s == "up" => Ok(()),
            Step::Builtin(s) => bail!("unknown step `{s}` (expected preview, up, or run: <cmd>)"),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    #[serde(default = "default_plan_stage")]
    pub plan: Stage,
    #[serde(default = "default_apply_stage")]
    pub apply: Stage,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            plan: default_plan_stage(),
            apply: default_apply_stage(),
        }
    }
}

fn default_plan_stage() -> Stage {
    Stage {
        steps: vec![Step::Builtin("preview".into())],
    }
}
fn default_apply_stage() -> Stage {
    Stage {
        steps: vec![Step::Builtin("up".into())],
    }
}

fn default_true() -> bool {
    true
}
fn default_when_modified() -> Vec<String> {
    vec!["**/*".into()]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AutoplanConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Globs relative to the project dir. `../shared/**` style paths are allowed.
    #[serde(default = "default_when_modified")]
    pub when_modified: Vec<String>,
}

impl Default for AutoplanConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            when_modified: default_when_modified(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub name: Option<String>,
    pub dir: String,
    pub stack: String,
    #[serde(default)]
    pub autoplan: AutoplanConfig,
    #[serde(default)]
    pub apply_requirements: Vec<Requirement>,
    pub workflow: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoConfig {
    pub version: u32,
    #[serde(default)]
    pub projects: Vec<ProjectConfig>,
    #[serde(default)]
    pub workflows: HashMap<String, Workflow>,
}

/// A fully resolved project+stack that imulup can plan/apply.
#[derive(Debug, Clone)]
pub struct Project {
    pub name: Option<String>,
    pub dir: String,
    pub stack: String,
    pub autoplan: bool,
    pub when_modified: Vec<String>,
    pub apply_requirements: Vec<Requirement>,
    pub workflow: Workflow,
}

impl Project {
    /// Human-facing identifier: the configured name, or `dir:stack`.
    pub fn id(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("{}:{}", self.dir, self.stack),
        }
    }

    /// Comment args that select exactly this project.
    pub fn selector(&self) -> String {
        match &self.name {
            Some(n) => format!("-p {n}"),
            None => format!("-d {} -s {}", self.dir, self.stack),
        }
    }

    fn matcher(&self) -> Result<GlobSet> {
        let mut b = GlobSetBuilder::new();
        for pat in &self.when_modified {
            let joined = normalize(&format!("{}/{}", self.dir, pat))
                .with_context(|| format!("when_modified pattern `{pat}` escapes the repo"))?;
            b.add(GlobBuilder::new(&joined).literal_separator(true).build()?);
        }
        Ok(b.build()?)
    }

    pub fn is_modified(&self, files: &[String]) -> Result<bool> {
        let m = self.matcher()?;
        Ok(files.iter().any(|f| m.is_match(f)))
    }
}

/// Normalize a repo-relative path: strips `.`/empty segments, resolves `..`, returns `.` for root.
/// Returns None if the path escapes the repo root.
pub fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    })
}

impl RepoConfig {
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: RepoConfig = serde_saphyr::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported version {} (expected 1)", self.version);
        }
        let mut names = HashSet::new();
        let mut keys = HashSet::new();
        for p in &self.projects {
            let dir = normalize(&p.dir)
                .with_context(|| format!("project dir `{}` escapes the repo", p.dir))?;
            if p.stack.trim().is_empty() {
                bail!("project in `{dir}` has an empty stack");
            }
            if !keys.insert((dir.clone(), p.stack.clone())) {
                bail!(
                    "dir `{dir}` / stack `{}` is configured more than once",
                    p.stack
                );
            }
            if let Some(n) = &p.name {
                if n.is_empty() || n.starts_with('-') || n.contains(char::is_whitespace) {
                    bail!("invalid project name `{n}`");
                }
                if !names.insert(n.clone()) {
                    bail!("project name `{n}` is used more than once");
                }
            }
            if let Some(w) = &p.workflow
                && !self.workflows.contains_key(w)
            {
                bail!("project `{dir}` uses undefined workflow `{w}`");
            }
        }
        for (name, w) in &self.workflows {
            for s in w.plan.steps.iter().chain(&w.apply.steps) {
                s.validate().with_context(|| format!("workflow `{name}`"))?;
            }
        }
        Ok(())
    }

    pub fn projects(&self) -> Vec<Project> {
        self.projects
            .iter()
            .map(|p| Project {
                name: p.name.clone(),
                dir: normalize(&p.dir).expect("validated"),
                stack: p.stack.clone(),
                autoplan: p.autoplan.enabled,
                when_modified: p.autoplan.when_modified.clone(),
                apply_requirements: p.apply_requirements.clone(),
                workflow: p
                    .workflow
                    .as_ref()
                    .map(|w| self.workflows[w].clone())
                    .unwrap_or_default(),
            })
            .collect()
    }
}

/// Loads `imulup.yaml` from a checked-out repo, if present.
pub fn load(repo_root: &Path) -> Result<Option<RepoConfig>> {
    for name in CONFIG_FILES {
        let path = repo_root.join(name);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)?;
            return RepoConfig::parse(&text)
                .map(Some)
                .with_context(|| format!("invalid {name}"));
        }
    }
    Ok(None)
}

fn is_pulumi_project_dir(dir: &Path) -> bool {
    dir.join("Pulumi.yaml").is_file() || dir.join("Pulumi.yml").is_file()
}

/// Stacks with a `Pulumi.<stack>.yaml` config file in `dir`.
pub fn stacks_in_dir(dir: &Path) -> Vec<String> {
    let mut stacks = BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name
                .strip_suffix(".yaml")
                .or_else(|| name.strip_suffix(".yml"));
            if let Some(stack) = stem.and_then(|s| s.strip_prefix("Pulumi."))
                && !stack.is_empty()
            {
                stacks.insert(stack.to_string());
            }
        }
    }
    stacks.into_iter().collect()
}

fn auto_project(dir: String, stack: String) -> Project {
    Project {
        name: None,
        dir,
        stack,
        autoplan: true,
        when_modified: default_when_modified(),
        apply_requirements: vec![],
        workflow: Workflow::default(),
    }
}

/// Without `imulup.yaml`: every stack of every Pulumi project that contains a modified file.
pub fn autodetect(repo_root: &Path, modified: &[String]) -> Vec<Project> {
    let mut dirs = BTreeSet::new();
    for file in modified {
        let mut cur = normalize(file).unwrap_or_default();
        loop {
            cur = match cur.rsplit_once('/') {
                Some((parent, _)) => parent.to_string(),
                None if cur == "." => break,
                None => ".".to_string(),
            };
            if is_pulumi_project_dir(&repo_root.join(&cur)) {
                dirs.insert(cur.clone());
                break;
            }
        }
    }
    dirs.into_iter()
        .flat_map(|d| {
            stacks_in_dir(&repo_root.join(&d))
                .into_iter()
                .map(move |s| auto_project(d.clone(), s))
        })
        .collect()
}

/// Every project imulup knows about in this checkout (used for explicit `-d/-s/-p` selection).
pub fn all_projects(
    repo_root: &Path,
    cfg: Option<&RepoConfig>,
    dir_hint: Option<&str>,
) -> Vec<Project> {
    if let Some(cfg) = cfg {
        return cfg.projects();
    }
    let Some(dir) = dir_hint.and_then(normalize) else {
        return vec![];
    };
    let path = repo_root.join(&dir);
    if !is_pulumi_project_dir(&path) {
        return vec![];
    }
    stacks_in_dir(&path)
        .into_iter()
        .map(|s| auto_project(dir.clone(), s))
        .collect()
}

/// Projects affected by `modified` files.
pub fn modified_projects(
    repo_root: &Path,
    cfg: Option<&RepoConfig>,
    modified: &[String],
) -> Result<Vec<Project>> {
    match cfg {
        None => Ok(autodetect(repo_root, modified)),
        Some(cfg) => {
            let mut out = Vec::new();
            for p in cfg.projects() {
                if p.is_modified(modified)? {
                    out.push(p);
                }
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize("./infra/").as_deref(), Some("infra"));
        assert_eq!(normalize(".").as_deref(), Some("."));
        assert_eq!(normalize("infra/../shared/x").as_deref(), Some("shared/x"));
        assert_eq!(normalize("../x"), None);
    }

    #[test]
    fn parses_full_config() {
        let cfg = RepoConfig::parse(
            r#"
version: 1
projects:
  - name: net-prod
    dir: ./network
    stack: prod
    apply_requirements: [approved, mergeable]
    autoplan:
      when_modified: ["**/*.ts", "../shared/**"]
    workflow: node
  - dir: app
    stack: dev
    autoplan:
      enabled: false
workflows:
  node:
    plan:
      steps:
        - run: npm ci
        - preview:
            extra_args: ["--refresh"]
    apply:
      steps:
        - run: npm ci
        - up
"#,
        )
        .unwrap();
        let ps = cfg.projects();
        assert_eq!(ps[0].id(), "net-prod");
        assert_eq!(ps[0].dir, "network");
        assert_eq!(
            ps[0].apply_requirements,
            vec![Requirement::Approved, Requirement::Mergeable]
        );
        assert_eq!(ps[0].workflow.plan.steps.len(), 2);
        assert_eq!(ps[1].id(), "app:dev");
        assert!(!ps[1].autoplan);
        assert_eq!(ps[1].workflow, Workflow::default());

        assert!(ps[0].is_modified(&["network/index.ts".into()]).unwrap());
        assert!(ps[0].is_modified(&["shared/lib/a.json".into()]).unwrap());
        assert!(!ps[0].is_modified(&["network/README.md".into()]).unwrap());
        assert!(ps[1].is_modified(&["app/src/x.py".into()]).unwrap());
        assert!(!ps[1].is_modified(&["application/x".into()]).unwrap());
    }

    #[test]
    fn rejects_invalid_configs() {
        let bad = [
            "version: 2",
            "version: 1\nprojects: [{dir: a, stack: dev}, {dir: ./a, stack: dev}]",
            "version: 1\nprojects: [{name: x, dir: a, stack: dev}, {name: x, dir: b, stack: dev}]",
            "version: 1\nprojects: [{dir: a, stack: dev, workflow: missing}]",
            "version: 1\nprojects: [{dir: ../a, stack: dev}]",
            "version: 1\nprojects: [{dir: a, stack: dev, typo: 1}]",
            "version: 1\nworkflows: {w: {plan: {steps: [destroy]}}}",
        ];
        for b in bad {
            assert!(RepoConfig::parse(b).is_err(), "should reject: {b}");
        }
    }

    #[test]
    fn autodetects_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("infra/src")).unwrap();
        std::fs::write(root.join("infra/Pulumi.yaml"), "name: infra").unwrap();
        std::fs::write(root.join("infra/Pulumi.dev.yaml"), "").unwrap();
        std::fs::write(root.join("infra/Pulumi.prod.yaml"), "").unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();

        let ps = autodetect(
            root,
            &["infra/src/index.ts".into(), "docs/readme.md".into()],
        );
        let ids: Vec<_> = ps.iter().map(Project::id).collect();
        assert_eq!(ids, vec!["infra:dev", "infra:prod"]);
        assert!(autodetect(root, &["docs/readme.md".into()]).is_empty());

        let ps = all_projects(root, None, Some("./infra"));
        assert_eq!(ps.len(), 2);
        assert!(all_projects(root, None, Some("docs")).is_empty());
    }

    #[test]
    fn autodetects_root_project() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Pulumi.yaml"), "name: x").unwrap();
        std::fs::write(tmp.path().join("Pulumi.staging.yaml"), "").unwrap();
        let ps = autodetect(tmp.path(), &["index.ts".into()]);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].id(), ".:staging");
    }
}
