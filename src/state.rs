//! Durable lock + plan state, persisted as JSON in the data dir.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Lock {
    pub repo: String,
    pub dir: String,
    pub stack: String,
    pub project: String,
    pub pull: u64,
    pub user: String,
    pub created_at: u64,
}

impl Lock {
    pub fn key(&self) -> String {
        lock_key(&self.repo, &self.dir, &self.stack)
    }
}

pub fn lock_key(repo: &str, dir: &str, stack: &str) -> String {
    format!("{repo}|{dir}|{stack}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanRecord {
    pub repo: String,
    pub pull: u64,
    pub project: String,
    pub dir: String,
    pub stack: String,
    pub head_sha: String,
    pub succeeded: bool,
    pub has_changes: Option<bool>,
    pub applied: bool,
}

impl PlanRecord {
    pub fn key(&self) -> String {
        plan_key(&self.repo, self.pull, &self.dir, &self.stack)
    }
}

pub fn plan_key(repo: &str, pull: u64, dir: &str, stack: &str) -> String {
    format!("{repo}#{pull}|{dir}|{stack}")
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    locks: BTreeMap<String, Lock>,
    plans: BTreeMap<String, PlanRecord>,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let data = if path.exists() {
            let text = std::fs::read_to_string(path)?;
            serde_json::from_str(&text)
                .with_context(|| format!("corrupt state file {}", path.display()))?
        } else {
            Data::default()
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path: path.to_path_buf(),
            data: Mutex::new(data),
        })
    }

    fn persist(&self, data: &Data) -> Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(data)?)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    fn with<T>(&self, f: impl FnOnce(&mut Data) -> T) -> Result<T> {
        let mut d = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let out = f(&mut d);
        self.persist(&d)?;
        Ok(out)
    }

    /// Acquires the lock, or returns the conflicting lock held by another PR.
    /// Re-locking by the same PR succeeds and keeps the original lock.
    pub fn try_lock(&self, lock: Lock) -> Result<std::result::Result<(), Box<Lock>>> {
        self.with(|d| match d.locks.get(&lock.key()) {
            Some(existing) if existing.pull != lock.pull => Err(Box::new(existing.clone())),
            Some(_) => Ok(()),
            None => {
                d.locks.insert(lock.key(), lock);
                Ok(())
            }
        })
    }

    pub fn get_lock(&self, repo: &str, dir: &str, stack: &str) -> Option<Lock> {
        let d = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.locks.get(&lock_key(repo, dir, stack)).cloned()
    }

    pub fn locks(&self) -> Vec<Lock> {
        let d = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.locks.values().cloned().collect()
    }

    /// Releases every lock and plan held by a PR; returns the released locks.
    pub fn release_pull(&self, repo: &str, pull: u64) -> Result<Vec<Lock>> {
        self.with(|d| {
            let released: Vec<Lock> = d
                .locks
                .values()
                .filter(|l| l.repo == repo && l.pull == pull)
                .cloned()
                .collect();
            d.locks.retain(|_, l| !(l.repo == repo && l.pull == pull));
            d.plans.retain(|_, p| !(p.repo == repo && p.pull == pull));
            released
        })
    }

    pub fn put_plan(&self, plan: PlanRecord) -> Result<()> {
        self.with(|d| {
            d.plans.insert(plan.key(), plan);
        })
    }

    pub fn get_plan(&self, repo: &str, pull: u64, dir: &str, stack: &str) -> Option<PlanRecord> {
        let d = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.plans.get(&plan_key(repo, pull, dir, stack)).cloned()
    }

    pub fn plans_for_pull(&self, repo: &str, pull: u64) -> Vec<PlanRecord> {
        let d = self
            .data
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        d.plans
            .values()
            .filter(|p| p.repo == repo && p.pull == pull)
            .cloned()
            .collect()
    }

    pub fn mark_applied(&self, repo: &str, pull: u64, dir: &str, stack: &str) -> Result<()> {
        self.with(|d| {
            if let Some(p) = d.plans.get_mut(&plan_key(repo, pull, dir, stack)) {
                p.applied = true;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock(pull: u64, stack: &str) -> Lock {
        Lock {
            repo: "o/r".into(),
            dir: "infra".into(),
            stack: stack.into(),
            project: format!("infra:{stack}"),
            pull,
            user: "u".into(),
            created_at: 0,
        }
    }

    #[test]
    fn locking_and_persistence() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        let s = Store::open(&path).unwrap();
        assert!(s.try_lock(lock(1, "dev")).unwrap().is_ok());
        assert!(s.try_lock(lock(1, "dev")).unwrap().is_ok());
        assert_eq!(s.try_lock(lock(2, "dev")).unwrap().unwrap_err().pull, 1);
        assert!(s.try_lock(lock(2, "prod")).unwrap().is_ok());
        s.put_plan(PlanRecord {
            repo: "o/r".into(),
            pull: 1,
            project: "infra:dev".into(),
            dir: "infra".into(),
            stack: "dev".into(),
            head_sha: "abc".into(),
            succeeded: true,
            has_changes: Some(true),
            applied: false,
        })
        .unwrap();
        s.mark_applied("o/r", 1, "infra", "dev").unwrap();

        let s2 = Store::open(&path).unwrap();
        assert_eq!(s2.locks().len(), 2);
        assert!(s2.get_plan("o/r", 1, "infra", "dev").unwrap().applied);

        let released = s2.release_pull("o/r", 1).unwrap();
        assert_eq!(released.len(), 1);
        assert_eq!(s2.plans_for_pull("o/r", 1), Vec::new());
        assert!(s2.try_lock(lock(2, "dev")).unwrap().is_ok());
    }
}
