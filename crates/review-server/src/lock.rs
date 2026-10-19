use crate::state_dir::StateDirectory;
use anyhow::{Result, bail};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

pub(crate) struct ReviewLock {
    file: File,
    path: PathBuf,
}

impl ReviewLock {
    pub fn acquire(directory: &Path) -> Result<Self> {
        loop {
            let path = StateDirectory::ensure(directory)?.join("lock");
            let file = match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if fs2::FileExt::try_lock_exclusive(&file).is_err() {
                bail!("This review is already running. Use its browser UI to conclude it.");
            }
            let lock = Self {
                file,
                path: path.clone(),
            };
            // A concluding process may unlink the lock between our open and lock calls.
            // Never open the database while holding a lock on that obsolete inode.
            if lock.is_current()? {
                return Ok(lock);
            }
        }
    }

    fn is_current(&self) -> Result<bool> {
        let metadata = match std::fs::metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let held = self.file.metadata()?;
            Ok(held.dev() == metadata.dev() && held.ino() == metadata.ino())
        }
        #[cfg(not(unix))]
        {
            let _ = metadata;
            Ok(true)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn obsolete_lock_is_not_reused_after_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let old = ReviewLock::acquire(dir.path()).unwrap();
        assert!(ReviewLock::acquire(dir.path()).is_err());
        std::fs::remove_file(&old.path).unwrap();
        let current = ReviewLock::acquire(dir.path()).unwrap();
        assert!(!old.is_current().unwrap());
        assert!(current.is_current().unwrap());
        StateDirectory::remove(dir.path()).unwrap();
    }
}
