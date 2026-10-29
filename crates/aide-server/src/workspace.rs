use crate::database::{Home, Lock, Project};
use aide_core::{Lifecycle, Repository, Task};
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

pub fn output(directory: &Path, program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(directory)
        .output()
        .with_context(|| format!("Run {program}; check it is installed"))?;
    if !output.status.success() {
        bail!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// All registrations/checkouts sharing Git's common directory share this lock.
/// jj and Git still own coordination with commands run outside Aide.
fn repository_lock(checkout: &Path) -> Result<Lock> {
    let common = output(
        checkout,
        "git",
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    Lock::wait(&Path::new(&common).join("aide-workspaces.lock"))
}

pub fn initialize(home: &Home, root: &Path, repositories: &[std::path::PathBuf]) -> Result<()> {
    let root = root.canonicalize()?;
    let mut validated = Vec::new();
    for repository in repositories {
        let checkout = if repository.is_absolute() {
            repository.clone()
        } else {
            root.join(repository)
        }
        .canonicalize()?;
        if checkout == root || !checkout.starts_with(&root) {
            bail!("Repository checkout must be inside the project root");
        }
        let git_root = output(&checkout, "git", &["rev-parse", "--show-toplevel"])?;
        if Path::new(&git_root).canonicalize()? != checkout {
            bail!("Specify the repository root, not a subdirectory");
        }
        let _repository = repository_lock(&checkout)?;
        if !checkout.join(".jj").exists() {
            output(&checkout, "jj", &["git", "init", "--colocate"])?;
        }
        output(&checkout, "jj", &["root"])?;
        let name = checkout.file_name().unwrap().to_string_lossy().into_owned();
        let tasks_dir = checkout.with_file_name(format!("{name}-tasks"));
        validated.push(Repository {
            name,
            checkout: checkout.display().to_string(),
            tasks_dir: tasks_dir.display().to_string(),
        });
    }
    let project = Project::init(&root)?;
    for repo in validated {
        project.add_repository(&repo)?;
    }
    home.register(&root)
}

pub fn valid_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 80
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!(
            "Task IDs must be 1–80 letters, digits, hyphens or underscores, starting with a letter or digit"
        );
    }
    Ok(())
}

pub fn start(project: &Project, id: &str, repository: Option<&str>) -> Result<Task> {
    valid_id(id)?;
    let transition = project.task_lock(id)?;
    prepare(project, id, repository, false, &transition)
}

/// Keep startup ownership across workspace creation and runtime launch.
pub fn start_managed(
    project: &Project,
    id: &str,
    repository: Option<&str>,
) -> Result<(Task, Lock)> {
    valid_id(id)?;
    let transition = project.task_lock(id)?;
    let task = prepare(project, id, repository, true, &transition)?;
    Ok((task, transition))
}

fn prepare(
    project: &Project,
    id: &str,
    repository: Option<&str>,
    managed: bool,
    _transition: &Lock,
) -> Result<Task> {
    let repositories = project.repositories()?;
    let repo = match repository {
        Some(name) => repositories
            .iter()
            .find(|r| r.name == name)
            .context("Unknown repository")?,
        None if repositories.len() == 1 => &repositories[0],
        _ => bail!("Choose a repository with --repo"),
    };
    let existing = project.tasks()?.into_iter().find(|t| t.id == id);
    if existing
        .as_ref()
        .is_some_and(|t| t.lifecycle != Lifecycle::Preparing)
    {
        bail!("Task ID already exists (including completed history)");
    }
    let checkout = Path::new(&repo.checkout);
    let _repository = repository_lock(checkout)?;
    // Fetch and resolve before creating bookkeeping or touching the task directory.
    output(checkout, "jj", &["git", "fetch", "--all-remotes"])?;
    let trunk = output(
        checkout,
        "jj",
        &[
            "log",
            "--no-graph",
            "-r",
            "trunk()",
            "-T",
            "commit_id ++ \"\\n\"",
        ],
    )?;
    if trunk.lines().count() != 1 || trunk.is_empty() || trunk.bytes().all(|b| b == b'0') {
        bail!("trunk() must resolve to one non-root commit");
    }
    fs::create_dir_all(&repo.tasks_dir)?;
    let parent = Path::new(&repo.tasks_dir).canonicalize()?;
    if parent != Path::new(&repo.tasks_dir) || !parent.starts_with(&project.root) {
        bail!("Task directory escapes the project root");
    }
    let mut task = existing.unwrap_or(Task {
        id: id.into(),
        key: uuid::Uuid::new_v4().simple().to_string(),
        repository: repo.name.clone(),
        workspace: parent.join(id).display().to_string(),
        lifecycle: Lifecycle::Preparing,
        desired_running: false,
        thread_id: None,
        native_path: None,
        recording: None,
        recovered: false,
        error: None,
        created: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    });
    if task.repository != repo.name {
        bail!("Retry using the original repository");
    }
    if managed {
        project.begin_startup(id)?;
    }
    let destination = Path::new(&task.workspace);
    if destination.exists() {
        // A crashed add may have completed before its final database write.
        verify_workspace(&task)?;
        let name = output(
            destination,
            "jj",
            &["workspace", "root", "--name", &task.session()],
        );
        if name.is_err() {
            bail!("Workspace path already exists and is not registered to this task");
        }
    } else {
        project.save(&task)?;
        if let Err(error) = output(
            checkout,
            "jj",
            &[
                "workspace",
                "add",
                "--colocate",
                "--name",
                &task.session(),
                "-r",
                &trunk,
                &task.workspace,
            ],
        ) {
            task.error = Some(error.to_string());
            project.save(&task)?;
            return Err(error);
        }
    }
    fs::write(destination.join(".jj/aide-owner"), &task.key)?;
    task.lifecycle = Lifecycle::Open;
    task.error = None;
    project.save(&task)?;
    if !managed {
        project.finish_startup(id)?;
    }
    Ok(task)
}

pub fn verify_workspace(task: &Task) -> Result<()> {
    let path = Path::new(&task.workspace);
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        bail!("Refusing a symlink workspace");
    }
    let owned = fs::read_to_string(path.join(".jj/aide-owner"))
        .ok()
        .as_deref()
        == Some(&task.key);
    if task.lifecycle == Lifecycle::Closing && owned {
        return Ok(());
    }
    let actual = output(
        path,
        "jj",
        &["workspace", "root", "--name", &task.session()],
    )?;
    if Path::new(&actual).canonicalize()? != path.canonicalize()? {
        bail!("Task workspace identity has changed");
    }
    if task.lifecycle != Lifecycle::Preparing && !owned {
        bail!("Task workspace ownership marker is missing");
    }
    Ok(())
}

pub fn remove(project: &Project, task: &mut Task) -> Result<()> {
    let repo = project
        .repositories()?
        .into_iter()
        .find(|r| r.name == task.repository)
        .context("Missing repository registration")?;
    let _repository = repository_lock(Path::new(&repo.checkout))?;
    let path = Path::new(&task.workspace);
    if path.parent() != Some(Path::new(&repo.tasks_dir))
        || path.file_name().and_then(|s| s.to_str()) != Some(&task.id)
    {
        bail!("Workspace path does not match task registration");
    }
    if path.exists() {
        verify_workspace(task)?;
    }
    let checkout = Path::new(&repo.checkout);
    let names = output(
        checkout,
        "jj",
        &["workspace", "list", "-T", "name ++ \"\\n\""],
    )?;
    if names.lines().any(|n| n == task.session()) {
        if path.exists() {
            output(path, "jj", &["status"])?;
        }
        output(checkout, "jj", &["workspace", "forget", &task.session()])?;
    }
    let git_worktrees = output(checkout, "git", &["worktree", "list", "--porcelain", "-z"])?;
    if git_worktrees
        .split('\0')
        .any(|entry| entry == format!("worktree {}", task.workspace))
    {
        output(
            checkout,
            "git",
            &["worktree", "remove", "--force", "--", &task.workspace],
        )?;
    }
    if path.exists() {
        fs::remove_dir_all(path)?;
    }
    *task = project.update(&task.id, |current| {
        current.lifecycle = Lifecycle::Completed;
        current.desired_running = false;
        current.error = None;
    })?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn fixture() -> Result<(tempfile::TempDir, Home, Project)> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("Cat Herder");
        fs::create_dir(&root)?;
        let source = temp.path().join("source");
        fs::create_dir(&source)?;
        output(&source, "git", &["init", "-b", "main"])?;
        fs::write(source.join("readme"), "trunk")?;
        output(&source, "git", &["add", "."])?;
        output(
            &source,
            "git",
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "Initial",
            ],
        )?;
        output(
            &root,
            "git",
            &["clone", source.to_str().unwrap(), "cat-herder"],
        )?;
        let home = Home::new(Some(temp.path().join("personal")))?;
        initialize(&home, &root, &["cat-herder".into()])?;
        let project = Project::open(&root)?;
        Ok((temp, home, project))
    }
    #[test]
    fn workspace_uses_trunk_is_discoverable_and_retains_history() -> Result<()> {
        let (_temp, _home, project) = fixture()?;
        let main = project.root.join("cat-herder");
        fs::write(main.join("readme"), "main dirty edits")?;
        let mut task = start(&project, "task-1", None)?;
        assert_eq!(
            fs::read_to_string(Path::new(&task.workspace).join("readme"))?,
            "trunk"
        );
        assert!(Path::new(&task.workspace).join(".git").exists());
        let nested = Path::new(&task.workspace).join("nested");
        fs::create_dir(&nested)?;
        assert_eq!(Project::discover(&nested)?.root, project.root);
        assert!(start(&project, "task-1", None).is_err());
        let second = start(&project, "task-2", None)?;
        task.lifecycle = Lifecycle::Closing;
        project.save(&task)?;
        remove(&project, &mut task)?;
        assert_eq!(project.task("task-1")?.lifecycle, Lifecycle::Completed);
        assert!(!Path::new(&task.workspace).exists());
        assert!(
            !output(&main, "git", &["worktree", "list", "--porcelain"])?.contains(&task.workspace)
        );
        assert!(Path::new(&second.workspace).exists());
        assert_eq!(fs::read_to_string(main.join("readme"))?, "main dirty edits");
        assert!(start(&project, "task-1", None).is_err());
        Ok(())
    }
    #[test]
    fn failed_creation_can_be_retried_and_path_traversal_is_rejected() -> Result<()> {
        let (_temp, _home, project) = fixture()?;
        for id in ["../outside", "-flag", "a/b", "", "task;echo"] {
            assert!(start(&project, id, None).is_err());
        }
        let mut task = start(&project, "retry", None)?;
        task.lifecycle = Lifecycle::Preparing;
        project.save(&task)?;
        let restored = start(&project, "retry", None)?;
        assert_eq!(task.key, restored.key);
        assert_eq!(restored.lifecycle, Lifecycle::Open);
        Ok(())
    }
    #[test]
    fn cleanup_resumes_after_jj_forget_and_preserves_unowned_paths() -> Result<()> {
        let (_temp, _home, project) = fixture()?;
        let mut task = start(&project, "interrupted", None)?;
        task.lifecycle = Lifecycle::Closing;
        project.save(&task)?;
        output(
            &project.root.join("cat-herder"),
            "jj",
            &["workspace", "forget", &task.session()],
        )?;
        remove(&project, &mut task)?;
        assert_eq!(task.lifecycle, Lifecycle::Completed);

        let mut other = start(&project, "unowned", None)?;
        fs::remove_file(Path::new(&other.workspace).join(".jj/aide-owner"))?;
        other.lifecycle = Lifecycle::Closing;
        assert!(remove(&project, &mut other).is_err());
        assert!(Path::new(&other.workspace).exists());
        Ok(())
    }

    #[test]
    fn missing_remote_and_ambiguous_repository_do_not_create_tasks() -> Result<()> {
        let (_temp, home, project) = fixture()?;
        let checkout = project.root.join("cat-herder");
        output(
            &checkout,
            "git",
            &["remote", "set-url", "origin", "/aide-test-does-not-exist"],
        )?;
        assert!(start(&project, "fetch-fails", None).is_err());
        assert!(project.tasks()?.is_empty());
        assert_eq!(home.roots()?.len(), 1);
        home.register(&project.root)?;
        assert_eq!(home.roots()?.len(), 1);
        Ok(())
    }

    #[test]
    fn unknown_marker_is_not_adopted() -> Result<()> {
        let temp = tempfile::tempdir()?;
        fs::create_dir(temp.path().join(".aide"))?;
        fs::write(temp.path().join(".aide/state.sqlite"), "not a database")?;
        assert!(Project::discover(temp.path()).is_err());
        assert!(Project::init(temp.path()).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join(".aide/state.sqlite"))?,
            "not a database"
        );
        Ok(())
    }
}
