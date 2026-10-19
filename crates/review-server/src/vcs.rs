use anyhow::{Context, Result, bail};
use review_core::{SourceFile, Workspace};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Command};

fn run(dir: &Path, program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .with_context(|| format!("Run {program}"))?;
    if !output.status.success() {
        bail!("{program}: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(output.stdout)
}
fn ignored(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.split('/')
        .any(|part| part == ".review" || part.starts_with(".review-cleanup-"))
        || name.starts_with("review.sqlite")
        || (name.starts_with("review-feedback-") && name.ends_with(".md"))
}
fn text(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) {
        None
    } else {
        String::from_utf8(bytes).ok()
    }
}
pub fn capture(dir: &Path) -> Result<Workspace> {
    let jj = run(dir, "jj", &["root"]).ok();
    let (vcs, root) = if let Some(root) = jj {
        ("jj", root)
    } else {
        (
            "git",
            run(dir, "git", &["rev-parse", "--show-toplevel"])
                .context("Start review inside a Git or jj workspace")?,
        )
    };
    let root = std::path::PathBuf::from(String::from_utf8(root)?.trim());
    let mut files = vec![];
    let mut diff = String::new();
    if vcs == "git" {
        let base = if run(&root, "git", &["rev-parse", "--verify", "HEAD"]).is_ok() {
            "HEAD"
        } else {
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        };
        let mut paths = run(&root, "git", &["diff", "--name-only", "-z", base, "--"])?;
        paths.extend(run(
            &root,
            "git",
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?);
        let mut paths: Vec<String> = paths
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| String::from_utf8(b.to_vec()))
            .collect::<Result<_, _>>()?;
        paths.sort();
        paths.dedup();
        for path in paths.into_iter().filter(|p| !ignored(p)) {
            let old_bytes = run(&root, "git", &["show", &format!("{base}:{path}")]).ok();
            let full = root.join(&path);
            let new_bytes = match std::fs::symlink_metadata(&full) {
                Ok(m) if m.file_type().is_symlink() => Some(
                    std::fs::read_link(&full)?
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                ),
                Ok(m) if m.is_file() => Some(std::fs::read(&full)?),
                _ => None,
            };
            let patch = run(
                &root,
                "git",
                &[
                    "-c",
                    "core.quotePath=false",
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--binary",
                    "--no-renames",
                    base,
                    "--",
                    &path,
                ],
            )?;
            diff.push_str(&String::from_utf8_lossy(&patch));
            let old = old_bytes.and_then(text);
            let new = new_bytes.clone().and_then(text);
            if patch.is_empty() {
                diff.push_str(&format!(
                    "diff --git a/{path} b/{path}\n--- /dev/null\n+++ b/{path}\n"
                ));
                if let Some(s) = &new {
                    diff.push_str(&format!("@@ -0,0 +1,{} @@\n", s.lines().count()));
                    for l in s.lines() {
                        diff.push('+');
                        diff.push_str(l);
                        diff.push('\n');
                    }
                } else {
                    diff.push_str(&format!(
                        "Binary file: {:x}\n",
                        Sha256::digest(new_bytes.unwrap_or_default())
                    ));
                }
            }
            files.push(SourceFile { path, old, new });
        }
    } else {
        let names = run(
            &root,
            "jj",
            &["diff", "--revision", "@", "--name-only", "--color", "never"],
        )?;
        for path in String::from_utf8(names)?.lines().filter(|p| !ignored(p)) {
            let old = run(&root, "jj", &["file", "show", "-r", "@-", path])
                .ok()
                .and_then(text);
            let new = run(&root, "jj", &["file", "show", "-r", "@", path])
                .ok()
                .and_then(text);
            diff.push_str(&String::from_utf8_lossy(&run(
                &root,
                "jj",
                &["diff", "--revision", "@", "--git", "--color", "never", path],
            )?));
            files.push(SourceFile {
                path: path.into(),
                old,
                new,
            });
        }
    }
    let mut hash = Sha256::new();
    hash.update(&diff);
    hash.update(serde_json::to_vec(&files)?);
    Ok(Workspace {
        fingerprint: format!("{:x}", hash.finalize()),
        diff,
        files,
        vcs: vcs.into(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captures_staged_unstaged_untracked_and_ignores_own_files() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path();
        run(p, "git", &["init", "-q"]).unwrap();
        std::fs::write(p.join("a"), "one\n").unwrap();
        run(p, "git", &["add", "a"]).unwrap();
        let w = capture(p).unwrap();
        assert_eq!(w.files[0].new.as_deref(), Some("one\n"));
        std::fs::write(p.join("a"), "two\n").unwrap();
        std::fs::write(p.join("new file"), "three\n").unwrap();
        std::fs::write(p.join("review.sqlite-wal"), "ignored").unwrap();
        std::fs::write(p.join("review-feedback-x.md"), "ignored").unwrap();
        let w2 = capture(p).unwrap();
        assert_eq!(w2.files.len(), 2);
        assert_ne!(w.fingerprint, w2.fingerprint);
        assert!(w2.diff.contains("+three"));
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;
    #[test]
    fn jj_working_commit_and_own_state_exclusion() {
        if Command::new("jj").arg("--version").output().is_err() {
            eprintln!("jj unavailable; skipping jj integration test");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let p = t.path();
        run(p, "jj", &["git", "init"]).unwrap();
        std::fs::write(p.join("hello.rs"), "fn main() {}\n").unwrap();
        std::fs::write(p.join("review.sqlite"), "state").unwrap();
        let w = capture(p).unwrap();
        assert_eq!(w.vcs, "jj");
        assert_eq!(w.files.len(), 1);
        assert_eq!(w.files[0].path, "hello.rs");
        assert!(w.diff.contains("+fn main() {}"));
        std::fs::write(p.join("review.sqlite"), "changed state").unwrap();
        assert_eq!(w.fingerprint, capture(p).unwrap().fingerprint);
    }
    #[test]
    fn tracked_deletions_modes_and_binary_changes() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path();
        run(p, "git", &["init", "-q"]).unwrap();
        std::fs::write(p.join("a"), "old\n").unwrap();
        std::fs::write(p.join("b"), [0, 1, 2]).unwrap();
        run(p, "git", &["add", "."]).unwrap();
        run(
            p,
            "git",
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "base",
            ],
        )
        .unwrap();
        std::fs::remove_file(p.join("a")).unwrap();
        std::fs::write(p.join("b"), [0, 2, 3]).unwrap();
        let w = capture(p).unwrap();
        assert_eq!(w.files.len(), 2);
        assert_eq!(w.files[0].old.as_deref(), Some("old\n"));
        assert!(w.files[0].new.is_none());
        assert!(w.diff.contains("GIT binary patch"));
    }
}

#[cfg(test)]
mod ignore_tests {
    use super::*;

    fn ignore_files(path: &Path) {
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join(".gitignore"), "*.log\n!important.log\n").unwrap();
        std::fs::write(path.join("nested/.gitignore"), "secret.txt\n").unwrap();
        std::fs::write(path.join("hidden.log"), "ignored").unwrap();
        std::fs::write(path.join("important.log"), "included").unwrap();
        std::fs::write(path.join("nested/secret.txt"), "ignored").unwrap();
        std::fs::write(path.join("nested/visible.txt"), "included").unwrap();
        std::fs::create_dir_all(path.join("nested/.review/diagrams")).unwrap();
        std::fs::write(path.join("nested/.review/diagrams/test.svg"), "internal").unwrap();
    }

    #[test]
    fn git_and_jj_respect_nested_ignore_rules_and_negation() {
        for vcs in ["git", "jj"] {
            if vcs == "jj" && Command::new("jj").arg("--version").output().is_err() {
                continue;
            }
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path();
            let args = if vcs == "git" {
                vec!["init", "-q"]
            } else {
                vec!["git", "init"]
            };
            run(path, vcs, &args).unwrap();
            ignore_files(path);
            let workspace = capture(path).unwrap();
            let paths: Vec<_> = workspace
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect();
            assert!(!paths.contains(&"hidden.log"));
            assert!(!paths.contains(&"nested/secret.txt"));
            assert!(paths.contains(&"important.log"));
            assert!(paths.contains(&"nested/visible.txt"));
            assert!(!paths.iter().any(|path| path.contains(".review/")));
            std::fs::write(
                path.join("nested/.review/diagrams/test.svg"),
                "updated internal data",
            )
            .unwrap();
            assert_eq!(workspace.fingerprint, capture(path).unwrap().fingerprint);
        }
    }
    #[test]
    fn already_tracked_changes_remain_visible_under_ignore_rules() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path();
        run(path, "git", &["init", "-q"]).unwrap();
        std::fs::write(path.join("tracked.log"), "before").unwrap();
        run(path, "git", &["add", "tracked.log"]).unwrap();
        std::fs::write(path.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(path.join("tracked.log"), "after").unwrap();
        assert!(
            capture(path)
                .unwrap()
                .files
                .iter()
                .any(|file| file.path == "tracked.log")
        );
    }
}
