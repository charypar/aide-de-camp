use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const INTERVAL: Duration = Duration::from_secs(10);

#[derive(PartialEq, Eq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl From<&fs::Metadata> for Stamp {
    fn from(m: &fs::Metadata) -> Self {
        Self {
            device: m.dev(),
            inode: m.ino(),
            size: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        }
    }
}

/// Cache metadata, not transcript contents. Scan at most once per interval;
/// shutdown/cleanup bypass the interval but still skip unchanged files.
#[derive(Default)]
pub struct Recorder {
    scans: HashMap<PathBuf, Instant>,
    files: HashMap<(PathBuf, PathBuf), (Stamp, Stamp)>,
}
impl Recorder {
    pub fn retain(&mut self, source: &Path, destination: &Path, final_copy: bool) -> Result<usize> {
        if !final_copy
            && self
                .scans
                .get(source)
                .is_some_and(|t| t.elapsed() < INTERVAL)
        {
            return Ok(0);
        }
        let count = self.copy_tree(source, destination)?;
        self.scans.insert(source.to_owned(), Instant::now());
        Ok(count)
    }
    fn copy_tree(&mut self, source: &Path, destination: &Path) -> Result<usize> {
        let metadata = fs::symlink_metadata(source)?;
        if metadata.file_type().is_symlink() {
            bail!("Refusing a symlink in a native recording");
        }
        if metadata.is_dir() {
            fs::create_dir_all(destination)?;
            let mut count = 0;
            for entry in fs::read_dir(source)? {
                let entry = entry?;
                count += self.copy_tree(&entry.path(), &destination.join(entry.file_name()))?;
            }
            return Ok(count);
        }
        if !metadata.is_file() {
            bail!("Native recording is not a regular file");
        }
        let key = (source.to_owned(), destination.to_owned());
        let source_stamp = Stamp::from(&metadata);
        let target = fs::symlink_metadata(destination).ok();
        if let Some(target) = &target
            && target.file_type().is_symlink()
        {
            bail!("Refusing a symlink at the recording destination");
        }
        if let Some((previous_source, previous_target)) = self.files.get(&key)
            && previous_source == &source_stamp
            && target
                .as_ref()
                .is_some_and(|m| previous_target == &Stamp::from(m))
        {
            return Ok(0);
        }
        let pending = tempfile::NamedTempFile::new_in(
            destination
                .parent()
                .context("Recording has no parent directory")?,
        )?;
        fs::copy(source, pending.path())?;
        pending.persist(destination)?;
        // If the native file changed during copying, don't mark that snapshot
        // as current. The next pass must read it again.
        if Stamp::from(&fs::symlink_metadata(source)?) == source_stamp {
            self.files.insert(
                key,
                (
                    source_stamp,
                    Stamp::from(&fs::symlink_metadata(destination)?),
                ),
            );
        } else {
            self.files.remove(&key);
        }
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_unchanged_files_and_retains_edits_growth_and_replacements() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("native");
        let target = temp.path().join("retained");
        fs::create_dir(&source)?;
        fs::write(source.join("a"), "aaa")?;
        fs::write(source.join("b"), "bbb")?;
        let mut recorder = Recorder::default();
        assert_eq!(recorder.retain(&source, &target, true)?, 2);
        let untouched_inode = fs::metadata(target.join("b"))?.ino();
        assert_eq!(recorder.retain(&source, &target, true)?, 0);
        fs::write(source.join("a"), "ccc")?;
        assert_eq!(recorder.retain(&source, &target, true)?, 1);
        assert_eq!(fs::metadata(target.join("b"))?.ino(), untouched_inode);
        fs::write(source.join("a"), "longer transcript")?;
        assert_eq!(recorder.retain(&source, &target, true)?, 1);
        fs::write(source.join("replacement"), "new")?;
        fs::rename(source.join("replacement"), source.join("a"))?;
        assert_eq!(recorder.retain(&source, &target, true)?, 1);
        fs::remove_file(target.join("b"))?;
        assert_eq!(recorder.retain(&source, &target, true)?, 1);
        assert_eq!(fs::read(target.join("a"))?, b"new");
        Ok(())
    }

    #[test]
    fn coalesces_active_updates_but_flushes_at_shutdown() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("events");
        let target = temp.path().join("retained");
        let mut recorder = Recorder::default();
        fs::write(&source, "first")?;
        assert_eq!(recorder.retain(&source, &target, false)?, 1);
        fs::write(&source, "latest")?;
        assert_eq!(recorder.retain(&source, &target, false)?, 0);
        assert_eq!(fs::read_to_string(&target)?, "first");
        assert_eq!(recorder.retain(&source, &target, true)?, 1);
        assert_eq!(fs::read_to_string(&target)?, "latest");
        recorder.scans.clear();
        assert_eq!(recorder.retain(&source, &target, false)?, 0);
        Ok(())
    }

    #[test]
    fn does_not_follow_links_and_simultaneous_copies_remain_complete() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let source = temp.path().join("events");
        let target = temp.path().join("retained");
        fs::write(&source, "event\n".repeat(1000))?;
        std::thread::scope(|scope| {
            let copies = (0..8)
                .map(|_| scope.spawn(|| Recorder::default().retain(&source, &target, true)))
                .collect::<Vec<_>>();
            for copy in copies {
                copy.join().unwrap().unwrap();
            }
        });
        assert_eq!(fs::read(&source)?, fs::read(&target)?);
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&source, &link)?;
        assert!(Recorder::default().retain(&link, &target, true).is_err());
        assert!(Recorder::default().retain(&source, &link, true).is_err());
        Ok(())
    }
}
