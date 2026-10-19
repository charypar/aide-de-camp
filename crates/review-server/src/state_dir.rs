use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};

const OWNER: &str = "DevDeck review state\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_legacy_data_and_respects_its_lock() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path();
        let legacy = directory.join("review.sqlite");
        let db = rusqlite::Connection::open(&legacy).unwrap();
        db.execute_batch("CREATE TABLE saved(value TEXT); INSERT INTO saved VALUES('draft');")
            .unwrap();
        drop(db);
        let lock = fs::File::create(directory.join("review.sqlite.lock")).unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        StateDirectory::ensure(directory).unwrap();
        assert!(StateDirectory::migrate_legacy(directory).is_err());
        assert!(legacy.exists());
        drop(lock);

        StateDirectory::migrate_legacy(directory).unwrap();
        assert!(!legacy.exists());
        assert!(!directory.join("review.sqlite.lock").exists());
        let db = rusqlite::Connection::open(StateDirectory::database(directory)).unwrap();
        assert_eq!(
            db.query_row("SELECT value FROM saved", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "draft"
        );
    }

    #[test]
    fn refuses_to_adopt_or_remove_an_unowned_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".review");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("notes"), "keep").unwrap();
        assert!(StateDirectory::ensure(temp.path()).is_err());
        assert!(StateDirectory::remove(temp.path()).is_err());
        assert_eq!(fs::read_to_string(path.join("notes")).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_state_directory_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        fs::write(other.path().join("owner"), OWNER).unwrap();
        std::os::unix::fs::symlink(other.path(), temp.path().join(".review")).unwrap();
        assert!(StateDirectory::ensure(temp.path()).is_err());
        assert!(StateDirectory::remove(temp.path()).is_err());
        assert!(other.path().join("owner").exists());
    }
}

pub(crate) struct StateDirectory;

impl StateDirectory {
    pub fn path(directory: &Path) -> PathBuf {
        directory.join(".review")
    }

    pub fn database(directory: &Path) -> PathBuf {
        Self::path(directory).join("review.sqlite")
    }

    pub fn exists(directory: &Path) -> bool {
        Self::database(directory).exists() || directory.join("review.sqlite").exists()
    }

    pub fn ensure(directory: &Path) -> Result<PathBuf> {
        let path = Self::path(directory);
        match fs::create_dir(&path) {
            Ok(()) => fs::write(path.join("owner"), OWNER)?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        Self::verify(&path)?;
        fs::write(path.join(".gitignore"), "*\n")?;
        Ok(path)
    }

    fn verify(path: &Path) -> Result<()> {
        if !fs::symlink_metadata(path)?.is_dir()
            || fs::read_to_string(path.join("owner")).ok().as_deref() != Some(OWNER)
        {
            bail!(
                "{} is not a DevDeck state directory; move it aside before starting a review",
                path.display()
            );
        }
        Ok(())
    }

    /// Called with the new directory's lock held, before opening the review database.
    pub fn migrate_legacy(directory: &Path) -> Result<()> {
        let legacy = directory.join("review.sqlite");
        if !legacy.exists() {
            return Ok(());
        }
        let lock_path = directory.join("review.sqlite.lock");
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("Stop the running legacy review before migrating its state")?;
        let destination = Self::database(directory);
        if destination.exists() {
            bail!(
                "Both review.sqlite and .review/review.sqlite exist; keep one review database before continuing"
            );
        }
        // Merge WAL data into the database before moving it; preserve resumable state.
        let db = rusqlite::Connection::open(&legacy)?;
        db.execute_batch("PRAGMA journal_mode=DELETE;")?;
        db.close().map_err(|(_, error)| error)?;
        fs::rename(&legacy, &destination)?;
        let temporary = directory.join("review.sqlite.export.tmp");
        if temporary.exists() {
            fs::rename(temporary, Self::path(directory).join("export.tmp"))?;
        }
        fs::remove_file(lock_path)?;
        Ok(())
    }

    pub fn remove(directory: &Path) -> Result<()> {
        let path = Self::path(directory);
        Self::verify(&path)?;
        // Detach the old directory atomically while its lock is still held. A new
        // process can then create .review without racing our recursive deletion.
        let retired = directory.join(format!(".review-cleanup-{}", uuid::Uuid::new_v4()));
        fs::rename(&path, &retired)?;
        fs::remove_dir_all(&retired)
            .with_context(|| format!("Feedback saved, but could not remove {}", retired.display()))
    }
}
