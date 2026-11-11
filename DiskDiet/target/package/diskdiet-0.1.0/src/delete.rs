//! Removing directories, including ones with read-only contents (the Go module
//! cache and some npm packages ship files and folders without write bits).

use std::fs;
use std::io;
use std::path::Path;

pub fn remove(path: &Path) -> io::Result<()> {
    // Refuse anything that is not a real directory: never follow a symlink
    // that was swapped in after the scan.
    let md = match fs::symlink_metadata(path) {
        Ok(md) => md,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    if !md.is_dir() {
        return Err(io::Error::other("not a directory"));
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            make_writable(path);
            fs::remove_dir_all(path)
        }
        Err(e) => Err(e),
    }
}

fn make_writable(path: &Path) {
    let Ok(md) = fs::symlink_metadata(path) else {
        return;
    };
    if md.file_type().is_symlink() {
        return;
    }
    let mut perms = md.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = perms.mode() | if md.is_dir() { 0o700 } else { 0o600 };
        perms.set_mode(mode);
    }
    #[cfg(not(unix))]
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    let _ = fs::set_permissions(path, perms);

    if md.is_dir()
        && let Ok(rd) = fs::read_dir(path)
    {
        for e in rd.flatten() {
            make_writable(&e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_read_only_trees() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("mod/pkg@v1.0.0");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("go.mod"), "module x").unwrap();
        let ro = |p: &Path| {
            let mut perm = fs::metadata(p).unwrap().permissions();
            perm.set_readonly(true);
            fs::set_permissions(p, perm).unwrap();
        };
        ro(&d.join("go.mod"));
        ro(&d);

        remove(&t.path().join("mod")).unwrap();
        assert!(!t.path().join("mod").exists());
    }

    #[test]
    fn refuses_files_and_tolerates_missing() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("file");
        fs::write(&f, "x").unwrap();
        assert!(remove(&f).is_err());
        assert!(f.exists());
        assert!(remove(&t.path().join("gone")).is_ok());
    }
}
