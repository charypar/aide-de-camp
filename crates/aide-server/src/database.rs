use aide_core::{Repository, Task};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&home.path, fs::Permissions::from_mode(0o700))?;
        }
        fs::create_dir_all(home.path.join("run"))?;
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
    pub fn task_lock(&self, id: &str) -> Result<Lock> {
        crate::workspace::valid_id(id)?;
        let directory = self.root.join(".aide/task-locks");
        fs::create_dir_all(&directory)?;
        Lock::acquire(&directory.join(format!("{id}.lock"))).with_context(|| {
            format!("Task {id} is busy; retry when its current operation finishes")
        })
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
