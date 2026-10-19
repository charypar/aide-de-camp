use crate::state_dir::StateDirectory;
use anyhow::{Result, bail};
use review_core::*;
use rusqlite::{Connection, OptionalExtension, params};
use std::{io::Write, path::Path};

pub struct Store {
    pub db: Connection,
    pub review: Review,
}

impl Store {
    fn fresh(directory: &std::path::Path, workspace: Workspace, notice: Option<String>) -> Review {
        Review {
            id: uuid::Uuid::new_v4().to_string(),
            directory: directory.to_string_lossy().into(),
            workspace,
            guide: String::new(),
            guide_html: String::new(),
            guide_warnings: vec![],
            messages: vec![],
            feedback: vec![],
            ui: UiState::default(),
            thread_id: None,
            turn_id: None,
            busy: false,
            generating: false,
            preparation_started_at_ms: None,
            requests: vec![],
            activity: vec![],
            status: ReviewStatus::Active,
            error: None,
            export_path: None,
            notice,
        }
    }

    pub fn open(directory: &Path, workspace: Workspace) -> Result<Self> {
        Self::load(directory, Some(workspace))
    }

    pub fn existing(directory: &Path) -> Result<Self> {
        if !StateDirectory::database(directory).exists() {
            bail!("No review exists in this directory. Run review first.");
        }
        Self::load(directory, None)
    }

    fn load(directory: &Path, workspace: Option<Workspace>) -> Result<Self> {
        StateDirectory::ensure(directory)?;
        let db = Connection::open(StateDirectory::database(directory))?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS reviews(id TEXT PRIMARY KEY, state TEXT NOT NULL, active INTEGER NOT NULL); CREATE TABLE IF NOT EXISTS commands(id TEXT PRIMARY KEY, review_id TEXT NOT NULL); CREATE TABLE IF NOT EXISTS events(seq INTEGER PRIMARY KEY, review_id TEXT NOT NULL, kind TEXT NOT NULL, payload TEXT NOT NULL);")?;
        let existing: Option<String> = db
            .query_row(
                "SELECT state FROM reviews WHERE active=1 LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let mut review = if let Some(s) = existing {
            serde_json::from_str::<Review>(&s)?
        } else {
            Self::fresh(
                directory,
                workspace
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("No active review in this directory"))?,
                None,
            )
        };
        if let Some(workspace) = workspace
            && review.workspace.fingerprint != workspace.fingerprint
        {
            review = Self::fresh(
                directory,
                workspace,
                Some(
                    "Workspace changed. Previous review is archived in .review/review.sqlite."
                        .into(),
                ),
            );
        }
        if review.busy {
            review.busy = false;
            review.generating = false;
            review.turn_id = None;
            review.requests.clear();
            review.error=Some("Agent work was interrupted when the server stopped. You can continue the conversation or retry guide generation.".into());
            for m in &mut review.messages {
                if m.status == MessageStatus::Streaming {
                    m.status = MessageStatus::Interrupted;
                }
            }
        }
        // HTML is a derived view. Re-render saved Markdown when presentation rules change.
        if !review.guide.is_empty() {
            (review.guide_html, review.guide_warnings) =
                document::render(&review.guide, &review.workspace);
        }
        let mut s = Self { db, review };
        s.save("open", None)?;
        Ok(s)
    }

    pub fn save(&mut self, kind: &str, command: Option<&Envelope>) -> Result<()> {
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE reviews SET active=0 WHERE active=1 AND id!=?",
            [&self.review.id],
        )?;
        tx.execute("INSERT INTO reviews VALUES(?1,?2,1) ON CONFLICT(id) DO UPDATE SET state=excluded.state,active=1",params![self.review.id,serde_json::to_string(&self.review)?])?;
        tx.execute(
            "INSERT INTO events(review_id,kind,payload) VALUES(?1,?2,?3)",
            params![
                self.review.id,
                kind,
                command
                    .map(serde_json::to_string)
                    .transpose()?
                    .unwrap_or_default()
            ],
        )?;
        if let Some(c) = command {
            tx.execute(
                "INSERT OR IGNORE INTO commands VALUES(?1,?2)",
                params![c.id, c.review_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn port(&self) -> Result<Option<u16>> {
        let p: Option<String> = self
            .db
            .query_row("SELECT value FROM settings WHERE key='port'", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(p.and_then(|p| p.parse().ok()))
    }

    pub fn set_port(&self, port: u16) -> Result<()> {
        self.db.execute("INSERT INTO settings VALUES('port',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[port.to_string()])?;
        Ok(())
    }

    pub fn seen(&self, id: &str) -> Result<bool> {
        Ok(self
            .db
            .query_row("SELECT 1 FROM commands WHERE id=?", [id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub fn conclude(&mut self) -> Result<()> {
        if self.review.status == ReviewStatus::Completed {
            return Ok(());
        }
        if self.review.busy {
            bail!("Stop the active Codex turn before concluding");
        }
        let path = Path::new(&self.review.directory)
            .join(format!("review-feedback-{}.md", self.review.id));
        let content = export_markdown(&self.review);
        // Publish a fully synced file atomically without replacing an existing handover.
        let temporary = StateDirectory::path(Path::new(&self.review.directory)).join("export.tmp");
        let mut f = std::fs::File::create(&temporary)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
        drop(f);
        let result = std::fs::hard_link(&temporary, &path);
        let _ = std::fs::remove_file(&temporary);
        match result {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read_to_string(&path)? != content {
                    bail!(
                        "Export file already exists with different content: {}",
                        path.display()
                    );
                }
            }
            Err(e) => return Err(e.into()),
        }
        #[cfg(unix)]
        std::fs::File::open(&self.review.directory)?.sync_all()?;
        self.review.status = ReviewStatus::Completed;
        self.review.export_path = Some(path.to_string_lossy().into());
        Ok(())
    }

    pub fn cleanup(&mut self) -> Result<()> {
        if self.review.status != ReviewStatus::Completed
            || !self
                .review
                .export_path
                .as_ref()
                .is_some_and(|path| Path::new(path).is_file())
        {
            bail!("Cannot remove review state before feedback has been exported");
        }
        // Close SQLite before unlinking its files, even while server tasks still hold App references.
        let db = std::mem::replace(&mut self.db, Connection::open_in_memory()?);
        if let Err((db, error)) = db.close() {
            self.db = db;
            return Err(error.into());
        }
        StateDirectory::remove(Path::new(&self.review.directory))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn w(s: &str) -> Workspace {
        Workspace {
            fingerprint: s.into(),
            diff: "".into(),
            files: vec![],
            vcs: "git".into(),
        }
    }
    #[test]
    fn restart_restores_drafts_feedback_and_interruptions() {
        let t = tempfile::tempdir().unwrap();
        let mut s = Store::open(t.path(), w("a")).unwrap();
        s.review.ui.chat_draft = "unfinished".into();
        s.review.feedback.push(Feedback {
            id: "f".into(),
            reference: None,
            text: "Keep my words\nexactly".into(),
        });
        s.review.busy = true;
        s.save("test", None).unwrap();
        drop(s);
        let s = Store::open(t.path(), w("a")).unwrap();
        assert_eq!(s.review.ui.chat_draft, "unfinished");
        assert!(!s.review.busy);
        assert!(s.review.error.is_some());
        assert_eq!(s.review.feedback.len(), 1);
    }
    #[test]
    fn reset_archives_and_export_is_idempotent() {
        let t = tempfile::tempdir().unwrap();
        let mut s = Store::open(t.path(), w("a")).unwrap();
        let old = s.review.id.clone();
        s.review.feedback.push(Feedback {
            id: "f".into(),
            reference: None,
            text: "literal **words**".into(),
        });
        s.conclude().unwrap();
        s.save("complete", None).unwrap();
        s.conclude().unwrap();
        assert!(
            std::fs::read_to_string(s.review.export_path.as_ref().unwrap())
                .unwrap()
                .contains("literal **words**")
        );
        drop(s);
        let s = Store::open(t.path(), w("b")).unwrap();
        assert_ne!(old, s.review.id);
        assert_eq!(
            s.db.query_row("SELECT count(*) FROM reviews", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn command_ids_survive_restart() {
        let t = tempfile::tempdir().unwrap();
        let mut s = Store::open(t.path(), w("a")).unwrap();
        let c = Envelope {
            id: "retry".into(),
            review_id: s.review.id.clone(),
            command: Command::Conclude,
        };
        s.save("command", Some(&c)).unwrap();
        drop(s);
        assert!(
            Store::open(t.path(), w("a"))
                .unwrap()
                .seen("retry")
                .unwrap()
        );
    }

    #[test]
    fn cleanup_requires_an_export_and_preserves_unrelated_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path(), w("cleanup")).unwrap();
        assert!(store.cleanup().is_err());
        assert!(StateDirectory::database(temp.path()).is_file());

        let unrelated = temp.path().join("review.sqlite.backup");
        std::fs::write(&unrelated, "keep").unwrap();
        std::fs::create_dir(StateDirectory::path(temp.path()).join("diagrams")).unwrap();
        std::fs::write(
            StateDirectory::path(temp.path()).join("diagrams/sketch.svg"),
            "temporary",
        )
        .unwrap();
        store.conclude().unwrap();
        store.save("complete", None).unwrap();
        let export = store.review.export_path.clone().unwrap();
        store.cleanup().unwrap();
        assert!(Path::new(&export).is_file());
        assert_eq!(std::fs::read_to_string(unrelated).unwrap(), "keep");
        assert!(!StateDirectory::path(temp.path()).exists());
    }

    #[test]
    fn missing_export_prevents_cleanup_of_completed_state() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path(), w("missing")).unwrap();
        store.conclude().unwrap();
        std::fs::remove_file(store.review.export_path.as_ref().unwrap()).unwrap();
        assert!(store.cleanup().is_err());
        assert!(StateDirectory::database(temp.path()).is_file());
    }

    #[test]
    fn preparation_clock_persists_and_old_reviews_still_load() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path(), w("clock")).unwrap();
        store.review.preparation_started_at_ms = Some(1000);
        store.save("clock", None).unwrap();
        drop(store);

        let store = Store::existing(temp.path()).unwrap();
        assert_eq!(
            store.review.preparation_elapsed(62000).as_deref(),
            Some("1:01")
        );
        assert_eq!(store.review.preparation_elapsed(0).as_deref(), Some("0:00"));

        let mut legacy = serde_json::to_value(&store.review).unwrap();
        legacy
            .as_object_mut()
            .unwrap()
            .remove("preparation_started_at_ms");
        let legacy: Review = serde_json::from_value(legacy).unwrap();
        assert_eq!(legacy.preparation_started_at_ms, None);
    }
}
