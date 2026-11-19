use aide_core::{Repository, Task};
use anyhow::{Context, Result, bail};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::Duration,
};

const APPLICATION: i32 = 0x41494445;

pub struct Lock(File);
impl Lock {
    fn file(path: &Path) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        Ok(file)
    }
    pub fn acquire(path: &Path) -> Result<Self> {
        let file = Self::file(path)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .context("Operation already in progress; try again")?;
        Ok(Self(file))
    }
    /// Blocking acquisition belongs on a blocking worker, never an async executor.
    pub fn wait(path: &Path) -> Result<Self> {
        let file = Self::file(path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(Self(file))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.0);
    }
}

#[derive(Clone)]
pub struct Home {
    pub path: PathBuf,
}
impl Home {
    pub fn new(path: Option<PathBuf>) -> Result<Self> {
        let path = path
            .or_else(|| std::env::var_os("AIDE_HOME").map(PathBuf::from))
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".aide")
            });
        fs::create_dir_all(&path)?;
        let home = Self {
            path: path.canonicalize()?,
        };
        fs::create_dir_all(home.path.join("run"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&home.path, fs::Permissions::from_mode(0o700))?;
            // Codex validates the socket's immediate parent, even when the
            // enclosing Aide home is private. Repair older installations too.
            fs::set_permissions(home.path.join("run"), fs::Permissions::from_mode(0o700))?;
        }
        fs::create_dir_all(home.path.join("recordings"))?;
        home.registry()?
            .execute_batch("CREATE TABLE IF NOT EXISTS projects(root TEXT PRIMARY KEY)")?;
        Ok(home)
    }
    fn registry(&self) -> Result<Connection> {
        let db = Connection::open(self.path.join("state.sqlite"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        Ok(db)
    }
    pub fn register(&self, root: &Path) -> Result<()> {
        self.registry()?.execute(
            "INSERT OR IGNORE INTO projects VALUES(?)",
            [root.to_string_lossy()],
        )?;
        Ok(())
    }
    pub fn instance_id(&self) -> Result<String> {
        let db = self.registry()?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS instance(singleton INTEGER PRIMARY KEY CHECK(singleton=1), id TEXT NOT NULL)")?;
        db.execute(
            "INSERT OR IGNORE INTO instance VALUES(1, ?)",
            [uuid::Uuid::new_v4().to_string()],
        )?;
        Ok(
            db.query_row("SELECT id FROM instance WHERE singleton=1", [], |row| {
                row.get(0)
            })?,
        )
    }
    pub fn roots(&self) -> Result<Vec<String>> {
        Ok(self
            .registry()?
            .prepare("SELECT root FROM projects ORDER BY root")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?)
    }
}

pub struct Project {
    pub root: PathBuf,
    db: Connection,
}
impl Project {
    pub fn init(root: &Path) -> Result<Self> {
        let root = root.canonicalize()?;
        let marker = root.join(".aide/state.sqlite");
        if marker.exists() {
            return Self::open(&root);
        }
        fs::create_dir_all(root.join(".aide"))?;
        let _lock = Lock::acquire(&root.join(".aide/operations.lock"))?;
        if marker.exists() {
            return Self::open(&root);
        }
        let db = Connection::open(&marker)?;
        db.execute_batch(&format!(
            "BEGIN; PRAGMA application_id={APPLICATION}; PRAGMA user_version=1;
            CREATE TABLE repositories(name TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE tasks(id TEXT PRIMARY KEY, body TEXT NOT NULL); COMMIT;"
        ))?;
        Self::open(&root)
    }
    pub fn open(root: &Path) -> Result<Self> {
        let root = root
            .canonicalize()
            .context("Project directory is unavailable")?;
        let db = Connection::open_with_flags(
            root.join(".aide/state.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .context("Not an Aide project; use aide init . --repo ./repository")?;
        db.busy_timeout(Duration::from_secs(5))?;
        let app: i32 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        let version: i32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if app != APPLICATION || version != 1 {
            bail!("Unrecognised Aide database or schema version");
        }
        // Additive metadata remains compatible with task workers from an older
        // installation, which may still rewrite the original task JSON.
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS task_summaries(id TEXT PRIMARY KEY, summary TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS task_startups(id TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS task_parking(id TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS task_parking_attention(id TEXT PRIMARY KEY, attention INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS github_links(task_id TEXT NOT NULL, url TEXT NOT NULL, body TEXT NOT NULL, PRIMARY KEY(task_id,url))",
        )?;
        Ok(Self { root, db })
    }
    pub fn discover(from: &Path) -> Result<Self> {
        for root in from.canonicalize()?.ancestors() {
            if root.join(".aide/state.sqlite").exists() {
                return Self::open(root);
            }
        }
        bail!(
            "No project found above {}; use aide init . --repo ./repository",
            from.display()
        )
    }
    fn decode_pr(url: &str, body: &str) -> Result<aide_core::PrView> {
        // The association key is independent of its cached observation. A corrupt
        // cache can be shown as unavailable and replaced by the next successful poll.
        let id = crate::github::parse_url(url).context("Invalid stored PR URL")?;
        if id.url() != url {
            bail!("Stored PR URL is not canonical");
        }
        if let Ok(view) = serde_json::from_str::<aide_core::PrView>(body)
            && view.id == id
        {
            return Ok(view);
        }
        Ok(aide_core::PrView {
            id,
            observation: None,
            last_attempt: 0,
            error: Some("Stored PR status could not be read; refresh to recover it".into()),
            stale: true,
            historical: false,
        })
    }
    pub fn pull_requests(&self, id: &str) -> Result<Vec<aide_core::PrView>> {
        let mut statement = self
            .db
            .prepare("SELECT url,body FROM github_links WHERE task_id=? ORDER BY rowid")?;
        let rows = statement.query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (url, body) = row?;
            Self::decode_pr(&url, &body)
        })
        .collect()
    }
    pub fn all_pull_requests(&self) -> Result<Vec<(String, Result<aide_core::PrView>)>> {
        let mut statement = self
            .db
            .prepare("SELECT task_id,url,body FROM github_links ORDER BY rowid")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (task, url, body) = row?;
            Ok((task, Self::decode_pr(&url, &body)))
        })
        .collect()
    }
    pub fn link_pr(&self, id: &str, view: &aide_core::PrView) -> Result<bool> {
        let _lock = self.task_lock(id)?;
        let transaction = self.transaction()?;
        if self.task(id)?.lifecycle != aide_core::Lifecycle::Open {
            bail!("Task is not open");
        }
        let added = self.db.execute(
            "INSERT OR IGNORE INTO github_links VALUES(?1,?2,?3)",
            params![id, view.id.url(), serde_json::to_string(view)?],
        )?;
        transaction.commit()?;
        Ok(added != 0)
    }
    pub fn unlink_pr(&self, id: &str, pr: &aide_core::PrId) -> Result<()> {
        let _lock = self.task_lock(id)?;
        let transaction = self.transaction()?;
        if self.task(id)?.lifecycle != aide_core::Lifecycle::Open {
            bail!("Task is not open");
        }
        self.db.execute(
            "DELETE FROM github_links WHERE task_id=? AND url=?",
            params![id, pr.url()],
        )?;
        transaction.commit()?;
        Ok(())
    }
    pub fn observe_pr(&self, id: &str, view: &aide_core::PrView) -> Result<()> {
        let transaction = self.transaction()?;
        if self.task(id)?.lifecycle == aide_core::Lifecycle::Open {
            self.db.execute(
                "UPDATE github_links SET body=? WHERE task_id=? AND url=?",
                params![serde_json::to_string(view)?, id, view.id.url()],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
    pub fn task_lock(&self, id: &str) -> Result<Lock> {
        crate::workspace::valid_id(id)?;
        let directory = self.root.join(".aide/task-locks");
        fs::create_dir_all(&directory)?;
        Lock::acquire(&directory.join(format!("{id}.lock"))).with_context(|| {
            format!("Task {id} is busy; retry when its current operation finishes")
        })
    }
    pub fn begin_startup(&self, id: &str) -> Result<()> {
        self.db
            .execute("INSERT OR IGNORE INTO task_startups VALUES(?)", [id])?;
        Ok(())
    }
    pub fn finish_startup(&self, id: &str) -> Result<()> {
        self.db
            .execute("DELETE FROM task_startups WHERE id=?", [id])?;
        Ok(())
    }
    pub fn startup_pending(&self, id: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_startups WHERE id=?)",
            [id],
            |row| row.get(0),
        )?)
    }
    pub fn task_busy(&self, id: &str) -> bool {
        self.task_lock(id).err().is_some_and(|error| {
            error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
        })
    }
    pub fn set_parked(&self, id: &str, parked: bool) -> Result<()> {
        if parked {
            return self.park_with_attention(id, false);
        }
        let tx = self.transaction()?;
        tx.execute("DELETE FROM task_parking WHERE id=?", [id])?;
        tx.execute("DELETE FROM task_parking_attention WHERE id=?", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn park_with_attention(&self, id: &str, attention: bool) -> Result<()> {
        let tx = self.transaction()?;
        tx.execute("INSERT OR IGNORE INTO task_parking VALUES(?)", [id])?;
        tx.execute(
            "INSERT OR REPLACE INTO task_parking_attention VALUES(?1,?2)",
            params![id, attention],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Persist the observed edge atomically with ending parking.
    pub fn observe_parking(&self, id: &str, attention: bool, eligible: bool) -> Result<bool> {
        let tx = self.transaction()?;
        let parked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_parking WHERE id=?)",
            [id],
            |r| r.get(0),
        )?;
        if !parked {
            return Ok(false);
        }
        let previous: bool = tx
            .query_row(
                "SELECT attention FROM task_parking_attention WHERE id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false);
        let remains_parked = eligible && (!attention || previous);
        if remains_parked {
            tx.execute(
                "INSERT OR REPLACE INTO task_parking_attention VALUES(?1,?2)",
                params![id, attention],
            )?;
        } else {
            tx.execute("DELETE FROM task_parking WHERE id=?", [id])?;
            tx.execute("DELETE FROM task_parking_attention WHERE id=?", [id])?;
        }
        tx.commit()?;
        Ok(remains_parked)
    }

    #[cfg(test)]
    pub fn is_parked(&self, id: &str) -> Result<bool> {
        Ok(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_parking WHERE id=?)",
            [id],
            |row| row.get(0),
        )?)
    }
    fn transaction(&self) -> Result<Transaction<'_>> {
        Ok(Transaction::new_unchecked(
            &self.db,
            TransactionBehavior::Immediate,
        )?)
    }
    pub fn repositories(&self) -> Result<Vec<Repository>> {
        self.read_all("repositories")
    }
    pub fn tasks(&self) -> Result<Vec<Task>> {
        self.read_all("tasks")
    }
    pub fn remember_summary(&self, id: &str, summary: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO task_summaries VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET summary=excluded.summary",
            params![id, summary],
        )?;
        Ok(())
    }
    pub fn task_summary(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row(
                "SELECT summary FROM task_summaries WHERE id=?",
                [id],
                |row| row.get(0),
            )
            .optional()?)
    }
    fn read_all<T: serde::de::DeserializeOwned>(&self, table: &str) -> Result<Vec<T>> {
        let values = self
            .db
            .prepare(&format!("SELECT body FROM {table} ORDER BY rowid"))?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|v| serde_json::from_str(&v).map_err(Into::into))
            .collect()
    }
    pub fn task(&self, id: &str) -> Result<Task> {
        let text: String = self
            .db
            .query_row("SELECT body FROM tasks WHERE id=?", [id], |r| r.get(0))
            .with_context(|| format!("Unknown task {id}"))?;
        Ok(serde_json::from_str(&text)?)
    }
    // Read-modify-write callers use a short immediate transaction. New task
    // insertion is protected by the project-local task lifecycle lock.
    pub fn save(&self, task: &Task) -> Result<()> {
        self.db.execute(
            "INSERT INTO tasks VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET body=excluded.body",
            params![task.id, serde_json::to_string(task)?],
        )?;
        Ok(())
    }
    /// First association wins. Keep the check and write in one transaction so
    /// simultaneous observers cannot adopt different conversations.
    pub fn bind_conversation(
        &self,
        id: &str,
        thread_id: &str,
        native_path: &str,
        recording: &str,
    ) -> Result<Task> {
        let transaction = self.transaction()?;
        let mut task = self.task(id)?;
        if task.lifecycle != aide_core::Lifecycle::Open {
            bail!("Cannot bind a conversation to a task that is not open");
        }
        if task
            .thread_id
            .as_deref()
            .is_some_and(|existing| existing != thread_id)
        {
            bail!(
                "Task already belongs to another conversation; refusing to replace its stored ID"
            );
        }
        if task.thread_id.as_deref() != Some(thread_id)
            || task.native_path.as_deref() != Some(native_path)
        {
            task.thread_id = Some(thread_id.into());
            task.native_path = Some(native_path.into());
            task.recording = Some(recording.into());
            self.save(&task)?;
        }
        transaction.commit()?;
        Ok(task)
    }
    pub fn add_repository(&self, repo: &Repository) -> Result<()> {
        let transaction = self.transaction()?;
        if let Some(existing) = self.repositories()?.iter().find(|r| r.name == repo.name) {
            if existing.checkout != repo.checkout {
                bail!("Repository name already registered");
            }
            transaction.commit()?;
            return Ok(());
        }
        self.db.execute(
            "INSERT INTO repositories VALUES(?,?)",
            params![repo.name, serde_json::to_string(repo)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Explicit user selection is the only operation allowed to replace a binding.
    /// Compare the previous identity inside the transaction; stale workers cannot
    /// undo this through the ordinary first-association path.
    pub fn repin_conversation(&self, previous: &Task, selected: &Task) -> Result<()> {
        let transaction = self.transaction()?;
        let mut current = self.task(&previous.id)?;
        if current.lifecycle != aide_core::Lifecycle::Open
            || current.key != previous.key
            || (current.thread_id != previous.thread_id
                && !(current.thread_id == selected.thread_id
                    && current.native_path == selected.native_path))
        {
            bail!("Task changed while selecting a conversation; try again");
        }
        if selected.thread_id.is_none() || selected.native_path.is_none() {
            bail!("Selected conversation has no durable history");
        }
        current.thread_id.clone_from(&selected.thread_id);
        current.native_path.clone_from(&selected.native_path);
        current.recording.clone_from(&selected.recording);
        current.error = None;
        self.save(&current)?;
        self.db
            .execute("DELETE FROM task_summaries WHERE id=?", [&current.id])?;
        transaction.commit()?;
        Ok(())
    }
    pub fn update(&self, id: &str, change: impl FnOnce(&mut Task)) -> Result<Task> {
        let transaction = self.transaction()?;
        let mut task = self.task(id)?;
        change(&mut task);
        self.save(&task)?;
        transaction.commit()?;
        Ok(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_socket_directory_is_private_on_creation_and_reopen() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("home");
        let home = Home::new(Some(path.clone()))?;
        let runtime = home.path.join("run");
        assert_eq!(fs::metadata(&runtime)?.permissions().mode() & 0o777, 0o700);
        fs::write(runtime.join("existing.log"), "preserve")?;
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o775))?;
        Home::new(Some(path))?;
        assert_eq!(fs::metadata(&runtime)?.permissions().mode() & 0o777, 0o700);
        assert_eq!(
            fs::read_to_string(runtime.join("existing.log"))?,
            "preserve"
        );
        Ok(())
    }
    use aide_core::Lifecycle;

    #[test]
    fn task_locks_are_project_local_and_independent() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = Project::init(temp.path())?;
        let another = Project::open(temp.path())?;
        let held = project.task_lock("one")?;
        assert!(another.task_lock("one").is_err());
        assert!(another.task_lock("two").is_ok());
        drop(held);
        assert!(another.task_lock("one").is_ok());
        Ok(())
    }

    #[test]
    fn concurrent_database_updates_do_not_lose_fields() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = Project::init(temp.path())?;
        let task = Task {
            id: "task".into(),
            key: "key".into(),
            repository: "repo".into(),
            workspace: temp.path().display().to_string(),
            lifecycle: Lifecycle::Open,
            desired_running: false,
            thread_id: None,
            native_path: None,
            recording: None,
            recovered: false,
            error: None,
            created: 0,
        };
        project.save(&task)?;
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let workers = (0..8)
                .map(|_| {
                    scope.spawn(|| -> Result<()> {
                        let project = Project::open(temp.path())?;
                        barrier.wait();
                        for _ in 0..20 {
                            project.update("task", |t| t.created += 1)?;
                        }
                        project.bind_conversation("task", "original", "/native", "/retained")?;
                        Ok(())
                    })
                })
                .collect::<Vec<_>>();
            for worker in workers {
                worker.join().unwrap().unwrap();
            }
        });
        let task = project.task("task")?;
        assert_eq!(task.created, 160);
        assert_eq!(task.thread_id.as_deref(), Some("original"));
        Ok(())
    }

    #[test]
    fn explicit_repin_is_atomic_and_stale_observers_cannot_undo_it() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let project = Project::init(temp.path())?;
        let mut task = Task {
            id: "task".into(),
            key: "key".into(),
            repository: "repo".into(),
            workspace: temp.path().display().to_string(),
            lifecycle: Lifecycle::Open,
            desired_running: true,
            thread_id: Some("original".into()),
            native_path: Some("/original".into()),
            recording: Some("/retained".into()),
            recovered: false,
            error: None,
            created: 0,
        };
        project.save(&task)?;
        project.remember_summary(&task.id, "Old title")?;
        let previous = task.clone();
        task.thread_id = Some("fork".into());
        task.native_path = Some("/fork".into());
        project.repin_conversation(&previous, &task)?;
        assert!(project.task_summary(&task.id)?.is_none());
        assert!(
            project
                .bind_conversation("task", "original", "/original", "/retained")
                .is_err()
        );
        assert!(project.repin_conversation(&previous, &previous).is_err());
        drop(project);
        let project = Project::open(temp.path())?;
        assert_eq!(project.task("task")?.thread_id.as_deref(), Some("fork"));
        // An older running observer may have discovered the exact new fork
        // before the CLI commits. Completing that same selection is safe.
        project.repin_conversation(&previous, &task)?;
        project.update("task", |t| t.lifecycle = Lifecycle::Completed)?;
        assert!(project.repin_conversation(&task, &previous).is_err());
        assert_eq!(project.task("task")?.thread_id.as_deref(), Some("fork"));
        Ok(())
    }

    #[test]
    fn conversation_binding_survives_reopen_and_cannot_switch_or_reopen_closed_task() -> Result<()>
    {
        let temp = tempfile::tempdir()?;
        let project = Project::init(temp.path())?;
        let task = Task {
            id: "task".into(),
            key: "key".into(),
            repository: "repo".into(),
            workspace: temp.path().display().to_string(),
            lifecycle: Lifecycle::Open,
            desired_running: false,
            thread_id: None,
            native_path: None,
            recording: None,
            recovered: false,
            error: None,
            created: 0,
        };
        project.save(&task)?;
        project.bind_conversation("task", "original", "/native", "/retained")?;
        drop(project);
        let project = Project::open(temp.path())?;
        assert!(
            project
                .bind_conversation("task", "another", "/other", "/retained")
                .is_err()
        );
        assert_eq!(project.task("task")?.thread_id.as_deref(), Some("original"));
        project.bind_conversation("task", "original", "/moved-native", "/retained")?;
        assert_eq!(
            project.task("task")?.native_path.as_deref(),
            Some("/moved-native")
        );
        project.update("task", |t| t.lifecycle = Lifecycle::Completed)?;
        assert!(
            project
                .bind_conversation("task", "original", "/native", "/retained")
                .is_err()
        );
        assert_eq!(project.task("task")?.lifecycle, Lifecycle::Completed);
        Ok(())
    }
}
