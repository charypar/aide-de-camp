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
        Self::database(directory).exists()
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

    pub fn remove(directory: &Path) -> Result<()> {
        let path = Self::path(directory);
        Self::verify(&path)?;
        // Detach the old directory atomically while its lock is still held. A new
        // process can then create .review without racing our recursive deletion.
        let retired = directory.join(format!(".review-cleanup-{}", uuid::Uuid::new_v4()));
        fs::rename(&path, &retired)?;
        fs::remove_dir_all(&retired).with_context(|| {
            format!(
                "Review concluded, but could not remove {}",
                retired.display()
            )
        })
    }
}
