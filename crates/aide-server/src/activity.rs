use crate::terminal::Terminal;
use aide_core::{Lifecycle, Task, TerminalActivity};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    process::Command,
};

/// Only process metadata is read: never command arguments or environment values.
#[derive(Default)]
pub struct ActivitySnapshot {
    sessions: HashMap<String, Option<TerminalActivity>>,
}

impl ActivitySnapshot {
    pub async fn capture() -> Self {
        tokio::task::spawn_blocking(Self::read)
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default()
    }

    fn read() -> Result<Self> {
        let panes = Terminal::run(&[
            "list-panes",
            "-a",
            "-F",
            "#{session_name}\t#{pane_id}\t#{pane_pid}\t#{pane_dead}\t#{pane_tty}\t#{@aide-backend}\t#{@aide-work}\t#{@aide-codex}",
        ])?;
        let panes = Pane::parse_all(&panes)?;
        if panes.is_empty() {
            return Ok(Self::default());
        }
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,tty=,stat=,lstart=,comm="])
            .env("LC_ALL", "C")
            .output()?;
        if !output.status.success() {
            bail!("Process observation unavailable");
        }
        let processes = Processes::parse(&String::from_utf8(output.stdout)?)?;
        Ok(Self::observe(&panes, &processes))
    }

    fn observe(panes: &[Pane], processes: &Processes) -> Self {
        let mut snapshot = Self::default();
        for pane in panes {
            let activity = pane.activity(processes);
            snapshot
                .sessions
                .entry(pane.session.clone())
                .and_modify(|current| {
                    *current = match (*current, activity) {
                        (Some(TerminalActivity::Working), _)
                        | (_, Some(TerminalActivity::Working)) => Some(TerminalActivity::Working),
                        (None, _) | (_, None) => None,
                        _ => Some(TerminalActivity::Idle),
                    };
                })
                .or_insert(activity);
        }
        snapshot
    }

    pub fn for_task(&self, task: &Task) -> Option<TerminalActivity> {
        if task.lifecycle == Lifecycle::Completed {
            return None;
        }
        self.sessions.get(&task.session()).copied().flatten()
    }
}

#[derive(Serialize, Deserialize)]
pub struct CodexProcess {
    pid: u32,
    started: String,
}

impl CodexProcess {
    pub fn remember() -> Result<()> {
        let pane = std::env::var("TMUX_PANE").context("Missing task pane")?;
        let pid = std::process::id();
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "lstart="])
            .env("LC_ALL", "C")
            .output()?;
        if !output.status.success() {
            bail!("Cannot identify Codex UI process");
        }
        let started = String::from_utf8(output.stdout)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if started.is_empty() {
            bail!("Missing Codex UI process start time");
        }
        let identity = serde_json::to_string(&Self { pid, started })?;
        Terminal::run(&["set-option", "-p", "-t", &pane, "@aide-codex", &identity])?;
        Ok(())
    }
}

struct Pane {
    session: String,
    pid: u32,
    dead: bool,
    tty: String,
    backend: bool,
    work: bool,
    codex: Option<CodexProcess>,
}

impl Pane {
    fn parse_all(text: &str) -> Result<Vec<Self>> {
        text.lines()
            .filter(|line| line.starts_with("aide-"))
            .map(|line| {
                // Terminal::run trims the trailing empty fields on the last row.
                let fields: Vec<_> = line.split('\t').collect();
                if fields.len() < 5 {
                    bail!("Incomplete tmux pane metadata");
                }
                Ok(Self {
                    session: fields[0].into(),
                    pid: fields[2].parse()?,
                    dead: fields[3] == "1",
                    tty: normalize_tty(fields[4]),
                    backend: fields.get(5).is_some_and(|id| *id == fields[1]),
                    work: fields.get(6).is_some_and(|id| *id == fields[1]),
                    codex: fields
                        .get(7)
                        .filter(|s| !s.is_empty())
                        .map(|s| serde_json::from_str(s))
                        .transpose()?,
                })
            })
            .collect()
    }

    fn activity(&self, processes: &Processes) -> Option<TerminalActivity> {
        if self.backend || self.dead {
            return Some(TerminalActivity::Idle);
        }
        // A live pane without its root process is a racing/incomplete snapshot,
        // not evidence that the terminal is idle.
        processes.items.get(&self.pid)?;
        let mut members = processes.descendants(self.pid);
        if !self.tty.is_empty() {
            members.extend(
                processes
                    .items
                    .values()
                    .filter(|p| p.tty == self.tty)
                    .map(|p| p.pid),
            );
        }
        let mut excluded = HashSet::new();
        if let Some(identity) = &self.codex
            && members.contains(&identity.pid)
            && processes
                .items
                .get(&identity.pid)
                .is_some_and(|p| p.started == identity.started)
        {
            excluded.extend(processes.descendants(identity.pid));
        }
        // Compatibility for native work panes started before process markers
        // existed. Deliberately do not classify Python/node wrappers by name.
        if self.work && self.codex.is_none() {
            for pid in &members {
                if processes.items[pid].name == "codex" {
                    excluded.extend(processes.descendants(*pid));
                }
            }
        }
        let working = members
            .iter()
            .filter(|pid| !excluded.contains(pid))
            .any(|pid| {
                let process = &processes.items[pid];
                !process.state.starts_with('Z') && !process.is_shell()
            });
        Some(if working {
            TerminalActivity::Working
        } else {
            TerminalActivity::Idle
        })
    }
}

struct Process {
    pid: u32,
    parent: u32,
    tty: String,
    state: String,
    started: String,
    name: String,
}

impl Process {
    fn is_shell(&self) -> bool {
        matches!(
            self.name.trim_start_matches('-'),
            "fish" | "sh" | "bash" | "zsh" | "dash" | "ksh" | "tcsh" | "csh" | "nu"
        )
    }
}

struct Processes {
    items: HashMap<u32, Process>,
    children: HashMap<u32, Vec<u32>>,
}

impl Processes {
    fn parse(text: &str) -> Result<Self> {
        let mut items = HashMap::new();
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for line in text.lines().filter(|s| !s.trim().is_empty()) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 10 {
                bail!("Incomplete process metadata");
            }
            let command = fields[9..].join(" ");
            let process = Process {
                pid: fields[0].parse()?,
                parent: fields[1].parse()?,
                tty: normalize_tty(fields[2]),
                state: fields[3].into(),
                started: fields[4..9].join(" "),
                name: Path::new(&command)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&command)
                    .into(),
            };
            children
                .entry(process.parent)
                .or_default()
                .push(process.pid);
            items.insert(process.pid, process);
        }
        Ok(Self { items, children })
    }

    fn descendants(&self, root: u32) -> HashSet<u32> {
        let mut result = HashSet::new();
        let mut pending = vec![root];
        while let Some(pid) = pending.pop() {
            if self.items.contains_key(&pid)
                && result.insert(pid)
                && let Some(children) = self.children.get(&pid)
            {
                pending.extend(children);
            }
        }
        result
    }
}

fn normalize_tty(tty: &str) -> String {
    match tty {
        "?" | "??" | "-" => String::new(),
        _ => tty
            .trim_start_matches("/dev/")
            .trim_start_matches("tty")
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent: u32, tty: &str, name: &str) -> String {
        format!("{pid} {parent} {tty} S Tue Oct 27 12:00:00 2026 {name}\n")
    }

    #[test]
    fn backend_is_excluded_by_pane_not_window_and_other_panes_still_count() -> Result<()> {
        let panes = Pane::parse_all(
            "aide-test\t%1\t10\t0\t/dev/ttys001\t%1\t%2\t\naide-test\t%2\t20\t0\t/dev/ttys002\t%1\t%2\t\naide-test\t%3\t30\t0\t/dev/ttys003\t%1\t%2\t",
        )?;
        let text = process(10, 1, "s001", "aide")
            + &process(11, 10, "s001", "codex")
            + &process(12, 11, "s001", "cargo")
            + &process(20, 1, "s002", "fish")
            + &process(30, 1, "s003", "fish");
        let idle = ActivitySnapshot::observe(&panes, &Processes::parse(&text)?);
        assert_eq!(idle.sessions["aide-test"], Some(TerminalActivity::Idle));
        // A background job counts, even after its parent exits, while it still
        // belongs to the pane's terminal. Processes in other terminals do not.
        let busy = text + &process(31, 1, "s003", "sleep") + &process(99, 1, "s999", "rustc");
        assert_eq!(
            ActivitySnapshot::observe(&panes, &Processes::parse(&busy)?).sessions["aide-test"],
            Some(TerminalActivity::Working)
        );
        Ok(())
    }

    #[test]
    fn codex_identity_excludes_wrapper_and_descendants_but_not_sibling_jobs() -> Result<()> {
        let marker = r#"{"pid":21,"started":"Tue Oct 27 12:00:00 2026"}"#;
        let panes = Pane::parse_all(&format!(
            "aide-test\t%2\t20\t0\t/dev/ttys002\t%1\t%2\t{marker}"
        ))?;
        let text = process(20, 1, "s002", "/opt/homebrew/bin/fish")
            + &process(21, 20, "s002", "/custom path/python3")
            + &process(22, 21, "s002", "node")
            + &process(23, 22, "s002", "cargo");
        assert_eq!(
            panes[0].activity(&Processes::parse(&text)?),
            Some(TerminalActivity::Idle)
        );
        let busy = text + &process(24, 20, "s002", "sleep");
        assert_eq!(
            panes[0].activity(&Processes::parse(&busy)?),
            Some(TerminalActivity::Working)
        );
        Ok(())
    }

    #[test]
    fn stale_process_marker_cannot_hide_new_work_and_missing_metadata_is_unknown() -> Result<()> {
        let marker = r#"{"pid":21,"started":"Mon Oct 26 12:00:00 2026"}"#;
        let panes = Pane::parse_all(&format!(
            "aide-test\t%2\t20\t0\t/dev/pts/2\t%1\t%2\t{marker}"
        ))?;
        let text = process(20, 1, "pts/2", "fish") + &process(21, 20, "pts/2", "sleep");
        assert_eq!(
            panes[0].activity(&Processes::parse(&text)?),
            Some(TerminalActivity::Working)
        );
        assert_eq!(panes[0].activity(&Processes::parse("")?), None);
        assert!(Processes::parse("garbled process row").is_err());
        Ok(())
    }

    #[test]
    fn legacy_native_codex_is_ignored_but_shell_work_after_exit_is_detected() -> Result<()> {
        let panes = Pane::parse_all("aide-test\t%2\t20\t0\t/dev/ttys002\t%1\t%2")?;
        let shell = process(20, 1, "s002", "fish");
        let native = shell.clone()
            + &process(21, 20, "s002", "/opt/bin/codex")
            + &process(22, 21, "s002", "node");
        assert_eq!(
            panes[0].activity(&Processes::parse(&native)?),
            Some(TerminalActivity::Idle)
        );
        assert_eq!(
            panes[0].activity(&Processes::parse(
                &(shell + &process(23, 20, "s002", "cargo"))
            )?),
            Some(TerminalActivity::Working)
        );
        Ok(())
    }

    #[test]
    fn dead_panes_and_zombies_do_not_mark_terminal_working() -> Result<()> {
        let panes = Pane::parse_all(
            "aide-test\t%3\t30\t1\t/dev/ttys003\t%1\t%2\t\naide-test\t%2\t20\t0\t/dev/ttys002\t%1\t%2\t",
        )?;
        let text =
            process(20, 1, "s002", "fish") + &process(21, 20, "s002", "gone").replace(" S ", " Z ");
        assert_eq!(
            ActivitySnapshot::observe(&panes, &Processes::parse(&text)?).sessions["aide-test"],
            Some(TerminalActivity::Idle)
        );
        Ok(())
    }
}
