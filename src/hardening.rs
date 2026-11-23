//! Process and file hardening that limits what pull request code can read from imulup.
//!
//! Pulumi programs and `run:` steps are arbitrary code running as the same OS user as imulup.
//! These checks don't sandbox them, but they close the easiest paths to imulup's credentials.

use std::path::Path;

use anyhow::Result;

/// Marks the process non-dumpable (Linux). Its `/proc/<pid>/environ` and `/proc/<pid>/mem` are
/// then readable only by root with `CAP_SYS_PTRACE`, and same-user processes can't ptrace it,
/// so steps can't lift the token, app key, or webhook secret from imulup's environment or
/// memory. Child processes are unaffected: dumpability is reset when they exec.
///
/// Returns whether the protection is in effect on this platform.
pub fn make_non_dumpable() -> Result<bool> {
    #[cfg(target_os = "linux")]
    {
        nix::sys::prctl::set_dumpable(false)?;
        Ok(true)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(false)
    }
}

/// Refuses a private key file that users other than its owner can access, like `ssh` does.
pub fn check_key_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            anyhow::bail!(
                "{} has permissions {mode:03o}, so other users can read it; \
                 run `chmod 600` on it (in Kubernetes, set the secret volume's defaultMode to 0400)",
                path.display()
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.pem");
        std::fs::write(&path, "k").unwrap();
        for (mode, ok) in [(0o600, true), (0o400, true), (0o640, false), (0o644, false)] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(check_key_file_permissions(&path).is_ok(), ok, "{mode:o}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn non_dumpable() {
        assert!(make_non_dumpable().unwrap());
        assert!(!nix::sys::prctl::get_dumpable().unwrap());
    }
}
