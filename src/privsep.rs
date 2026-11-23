//! Privilege separation between imulup and the pull request code it runs.
//!
//! Pulumi programs and `run:` steps are arbitrary code from pull requests. With
//! `--server-user` and `--step-user`, imulup starts as root, reads its secrets, prepares the data
//! directory, and then permanently becomes the server user. Checkouts and steps run as the step
//! user, which can't read imulup's key file, state, git mirrors' credentials, environment, or
//! memory, and can't signal imulup.
//!
//! The server user keeps exactly three capabilities, and only when there is a step user:
//! `CAP_SETUID` and `CAP_SETGID` to start processes as the step user, and `CAP_KILL` to stop
//! steps that time out. Steps get none: capabilities are dropped when they exec, and
//! `no_new_privs` stops them from regaining any through setuid binaries such as `sudo`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A local user, resolved from its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
}

impl Identity {
    pub fn lookup(name: &str) -> Result<Self> {
        let u = nix::unistd::User::from_name(name)
            .with_context(|| format!("looking up user {name:?}"))?
            .with_context(|| format!("no such user {name:?}"))?;
        Ok(Self {
            name: u.name,
            uid: u.uid.as_raw(),
            gid: u.gid.as_raw(),
            home: u.dir,
        })
    }
}

pub fn is_root() -> bool {
    nix::unistd::geteuid().is_root()
}

/// Creates the data directory layout with ownership and modes for privilege separation. Runs as
/// root, before [`drop_privileges`].
///
/// * `<data_dir>`: server user, `0711`, so the step user can reach its own subdirectories but
///   can't list the rest
/// * `<data_dir>/state.json`: server user, `0600`
/// * `<data_dir>/mirrors`: server user, `0755`, read-only to the step user
/// * `<data_dir>/repos`: step user, `0755`, readable by imulup for `imulup.yaml`
///
/// Trees left by an earlier version (or a different user) are re-owned.
pub fn prepare_data_dir(data_dir: &Path, server: &Identity, step: Option<&Identity>) -> Result<()> {
    std::fs::create_dir_all(data_dir)?;
    own(data_dir, server, if step.is_some() { 0o711 } else { 0o700 })?;
    for name in ["state.json", "state.json.tmp"] {
        let path = data_dir.join(name);
        if let Ok(meta) = path.symlink_metadata() {
            // chmod follows symlinks, and earlier versions ran PR code with write access here.
            anyhow::ensure!(meta.is_file(), "{} is not a regular file", path.display());
            own(&path, server, 0o600)?;
        }
    }
    let mirrors = data_dir.join("mirrors");
    std::fs::create_dir_all(&mirrors)?;
    own_tree(&mirrors, server)?;
    own(&mirrors, server, 0o755)?;

    let repos = data_dir.join("repos");
    std::fs::create_dir_all(&repos)?;
    let repo_owner = step.unwrap_or(server);
    own_tree(&repos, repo_owner)?;
    own(&repos, repo_owner, 0o755)?;
    Ok(())
}

fn own(path: &Path, id: &Identity, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::os::unix::fs::lchown(path, Some(id.uid), Some(id.gid))
        .with_context(|| format!("chown {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

/// Re-owns `root` and everything under it, without following symlinks. Skips the walk when
/// `root` already has the right owner, so restarts don't re-walk large checkouts.
fn own_tree(root: &Path, id: &Identity) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if root.symlink_metadata()?.uid() == id.uid {
        return Ok(());
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let meta = path.symlink_metadata()?;
        std::os::unix::fs::lchown(&path, Some(id.uid), Some(id.gid))
            .with_context(|| format!("chown {}", path.display()))?;
        if meta.is_dir() {
            for entry in std::fs::read_dir(&path)? {
                stack.push(entry?.path());
            }
        }
    }
    Ok(())
}

/// Permanently switches the process from root to `server`. With `keep_step_caps`, keeps only
/// what's needed to run and stop steps as another user (see the module docs).
///
/// Capabilities are per-thread on Linux, so this must run before any other thread starts, in
/// particular before the async runtime is built. It checks this and fails otherwise.
#[cfg(target_os = "linux")]
pub fn drop_privileges(server: &Identity, keep_step_caps: bool) -> Result<()> {
    use caps::{CapSet, Capability, CapsHashSet};
    use nix::unistd::{Gid, Uid, setgroups, setresgid, setresuid};

    anyhow::ensure!(is_root(), "--server-user needs imulup to start as root");
    let threads = std::fs::read_dir("/proc/self/task")?.count();
    anyhow::ensure!(
        threads == 1,
        "dropping privileges needs a single-threaded process, found {threads} threads"
    );

    let keep: CapsHashSet = if keep_step_caps {
        [
            Capability::CAP_SETUID,
            Capability::CAP_SETGID,
            Capability::CAP_KILL,
        ]
        .into_iter()
        .collect()
    } else {
        CapsHashSet::new()
    };
    // The bounding set caps what any later exec can gain; shrink it while we still can.
    for cap in caps::read(None, CapSet::Bounding)? {
        if !keep.contains(&cap) {
            caps::drop(None, CapSet::Bounding, cap)?;
        }
    }
    caps::clear(None, CapSet::Inheritable)?;
    caps::clear(None, CapSet::Ambient)?;

    let (uid, gid) = (Uid::from_raw(server.uid), Gid::from_raw(server.gid));
    // Keep permitted capabilities across the uid change; the effective set is cleared by it.
    nix::sys::prctl::set_keepcaps(true)?;
    setgroups(&[])?;
    setresgid(gid, gid, gid)?;
    setresuid(uid, uid, uid)?;
    nix::sys::prctl::set_keepcaps(false)?;
    caps::set(None, CapSet::Permitted, &keep)?;
    caps::set(None, CapSet::Effective, &keep)?;
    nix::sys::prctl::set_no_new_privs()?;
    // Changing uid can reset dumpability; make sure it stays off.
    crate::hardening::make_non_dumpable()?;

    anyhow::ensure!(
        Uid::current() == uid && Uid::effective() == uid && Gid::effective() == gid,
        "failed to switch to user {}",
        server.name
    );
    anyhow::ensure!(
        caps::read(None, CapSet::Permitted)? == keep,
        "failed to drop capabilities"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn drop_privileges(_server: &Identity, _keep_step_caps: bool) -> Result<()> {
    anyhow::bail!("--server-user is only supported on Linux")
}
