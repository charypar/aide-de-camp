# Aide de camp

> **Big honking warning**: This is entirely vibe coded, I haven't even read the code, and might not until I feel like it.
> It's very much build for my preferred tools and workflow – codex, tmux, Alacritty, Zed – therefore unlikely to work
> for yours.
>
> It's likely that the whole thing is quite rough, and will change a lot. Try at your own risk. You've been warned.

AI + Development Environment, and an aide-de-camp for development work. Aide is the short name; the coordination CLI is `aide`, and the independent review CLI is `aide-review`.

A local task workbench with an independent guided review tool. Codex narrates your current Git uncommitted changes or jj working commit. Select guide text or source lines to discuss them, record explicit feedback, and submit one Markdown handover.

The Cargo workspace contains both tools. See the [coordination plan](plans/COORDINATION_PLAN.md) and [review plan](plans/REVIEW_PLAN.md) for their scope.

## Task workspaces

Install both commands with `just install`. Aide also needs jj, Git, tmux, fish, Codex, Alacritty, and Zed installed. Start from a project container with its repository checkout inside it:

```sh
cd ~/Projects/CatHerder
aide init . --repo ./cat-herder
aide                         # Start the local dashboard
```

Use **New task** in the dashboard to create a colocated jj/Git workspace from fetched `trunk()` and open an idle Codex conversation in its own Alacritty window. Or, in another terminal inside the project:

```sh
aide task start task-1234
aide task attach task-1234
aide task attach task-1234 --new-window
aide task list
aide task review task-1234
aide task cleanup task-1234
```

Independent task operations run concurrently. Conflicting operations on the same task report that it is busy; retry after the current operation finishes. Workspace mutations sharing a repository wait their turn, while other repositories remain usable.

`task start` and runtime repair need dashboard mode running. Attaching to a healthy task or resuming its live backend works without it. Inside tmux, attach switches the current client; outside it, attach connects normally. Ctrl-b then d, `aide task detach`, or closing the dedicated Alacritty window disconnects you and leaves the task running.

Each task has two tmux windows:

- **work:** fish with the native Codex UI. Exit Codex to return to fish; run `aide codex resume` to reconnect in that same terminal. Exiting the UI leaves the backend running.
- **backend:** the task worker, dedicated Codex server, and recording monitor. Ctrl-C here stops the backend and interrupts active work; its stopped output remains visible.

With the usual tmux bindings, Ctrl-b then w selects a window; Ctrl-b then c creates another for jj commands or a local server. You can rename/reorder windows. `aide codex resume` works from any window in the task session, including after changing directories. From another terminal, first use `aide task attach <id>`. A directory belonging to a different task is rejected.

To refresh the environment available to agent tools: exit the Codex UI, stop the backend with Ctrl-C in its window, return to the work shell, change exported variables, then run `aide codex resume`. With the dashboard running, this recreates the backend from that shell's exported environment, including removed variables, and resumes the saved conversation idle. Resuming a healthy backend simply reconnects; it does not refresh its environment.

Stopping Aide leaves task sessions running. Dashboard startup restores missing task sessions after a reboot, with saved conversations idle and requiring attention; it never replays interrupted commands. A never-used Codex conversation may reopen fresh because it has no durable recording yet. If you stop/delete only the backend window, use `aide codex resume`, `aide task attach <id>`, or **Recover session** to repair it. Recovery preserves user-added windows and refuses to replace unrelated foreground commands. After a reboot it restores Aide's windows, not arbitrary local-server commands you launched yourself.

Closing a task requires confirmation, warns about live sessions, ends the entire task tmux session (including your extra windows), and removes the workspace. Publish your work manually before closing. History and recordings remain available under **Show completed**.

A short first test, after initialization and starting `aide` in another terminal:

```sh
aide task start trial-1 --new-window
# In the new terminal: have a short Codex conversation, then exit Codex.
aide codex resume             # In that task's fish shell
# Detach with Ctrl-b then d; back in the project terminal:
aide task attach trial-1 --new-window
aide task review trial-1       # After making local changes
# Back in the project terminal, when the workspace is disposable:
aide task cleanup trial-1      # Type trial-1 to confirm deletion
aide task list --completed
```

For workspace management alone, `aide task start task-1234 --workspace-only` does not need a dashboard or start an agent. Repeat `aide init . --repo ./another-repo` to register another repository; use `--repo <name>` when starting tasks in projects with several repositories. Task IDs are unique within a project, including completed history.

Personal registry and native recording copies live under `~/.aide`; project records live in `.aide/state.sqlite`. `--home` or `AIDE_HOME` selects a separate personal data directory. Codex keeps its normal configuration, authentication and original native sessions. Once a conversation has saved history, its ID is bound to the task; starting another conversation in the same folder does not switch that association. Recording copies check for changes every ten seconds, skip unchanged files, and flush on orderly shutdown/cleanup. The optional conversation label comes from Codex's native thread name; Aide does not generate summaries. Task monitoring reads native status without taking over approvals, which stay in Codex. The app-server transport is experimental and was checked with Codex 0.163.0.

`aide --no-open --port 4318` serves the dashboard without opening the browser. It listens only on localhost. `aide doctor` checks external commands. `AIDE_CODEX` and `AIDE_TMUX_SOCKET` allow isolated harness/tmux configurations, primarily for acceptance tests.

## Start developing

Prerequisites: Rust via [rustup](https://rustup.rs), a configured and signed-in Codex CLI, and Git or jj. Install [Just](https://github.com/casey/just) with `brew install just` on macOS or `cargo install just --locked`.

```sh
just setup
just dev /path/to/repository
```

Open the printed local URL. `just dev` uses the Rust `xtask` runner to watch Rust and UI sources (no Python needed); refresh after a UI build. `just --list` documents the commands. Browser testing dependencies are optional development tooling, installed by `just setup-browser`.

## Install and use

```sh
just install
cd /path/to/repository
aide-review
```

The installed CLI embeds the UI and SQLite; it needs no Trunk or Node runtime. It launches the already-installed Codex harness with your normal configuration and authentication. The browser opens when the guide is ready; approval/input requests and preparation failures open it earlier so you can respond. The URL is printed immediately if you want to open it yourself. Review sessions use a read-only sandbox with approval requests shown in the UI. You can interrupt work and answer native harness requests in the browser. Pending approvals and questions appear at the top of the side panel, including while the initial guide is being prepared.

Preparation shows an elapsed clock in the terminal and browser, with a distinct status when Codex is waiting for your response. Browser refresh preserves the clock; retry starts a new attempt. Elapsed time includes approval waits and is not a completion estimate. Redirected terminal output reports state changes and progress every 15 seconds.

`aide-review --no-open --port 4317` prints the URL without opening a browser. `aide-review --path /path/to/repository` starts elsewhere. Stop with Ctrl-C and run again to resume. The server remembers its port so existing browser tabs can reconnect; use `--port 0` to choose a different port if necessary. To conclude without the browser, first stop the server, then run `aide-review conclude`. `aide-review history` lists archived reviews until conclusion cleans up the local database. `aide-review --fixture` provides a clearly deterministic harness for UI development without model calls.

`aide review` delegates to the same executable in the current repository; `aide task review <id>` locates the task workspace first. Both preserve the reviewer’s options and subcommands. Review feedback remains a Markdown handover.

## State and feedback

Application state lives in `.review/` in the starting directory: `review.sqlite`, the lock, and temporary files. The folder is ignored by the review input and contains its own Git ignore file. Add `.review/` and `review-feedback-*.md` to the repository's ignore file if desired.

The complete repository workspace is reviewed even when launched in a subdirectory. Git input combines staged, unstaged, and untracked changes against HEAD. In jj it uses the working commit `@`. Both respect `.gitignore` for untracked files, including nested rules and negation; already tracked changes remain part of the review, matching VCS behavior. Source mappings are verified against a captured snapshot; binary changes are represented in the diff and described in prose.

The workspace is captured once when `aide-review` starts. Edits during a running session do not reset it; stop and relaunch to review changed code. On relaunch, changed code archives the previous review and automatically prepares a fresh guide. Archived reviews remain in SQLite until conclusion; this first milestone does not rebase feedback onto evolving code. Successful conclusion removes the whole `.review/` folder, including temporary assets, leaving the Markdown feedback file in the starting directory. A later `aide-review` run starts fresh, even on unchanged code. Failed exports preserve review state. Each page represents one review, with no in-page restart or regeneration controls. If preparation fails or is interrupted, **Retry preparation** retries the missing guide. `aide-review conclude` exports the saved review even if code has changed since it stopped.

The walkthrough sits beside a collapsible chat panel; the panel icon beside the Aide name toggles it. Select guide prose or code to reveal two actions: **Add feedback** opens a wide, flat comment editor in the right margin, flush with the page edge; **Discuss with agent** opens chat, attaches that reference, and focuses the composer. Selecting alone does not change chat context. Recorded feedback stays highlighted until deleted; clicking a highlight focuses its comment. Comments remain editable, and overlapping comments stack without covering each other. Inline editors have a white background; opening one preserves the document’s reading position.

Drag or triple-click to select prose; drag across code rows or click a row then Shift-click another to select a range. Sent chat messages show the quoted passage or captured code excerpt. Enter sends chat or records an inline comment; Shift+Enter inserts a newline. Use **Reference a file range** in chat for changes absent from the guide, including ranges found in Zed. Source references include an old/new side for deleted lines. **Copy reference** copies `path:line` for opening in your editor. Chat follows streamed text while you are at the bottom; scrolling up pauses following until you return to the bottom or use **Jump to latest**. Pending requests remain signalled in the header when chat is collapsed.

Inline comments have a **Change / Question** toggle. The collapsible bottom drawer contains one uncategorised **Overall feedback** field, saved as you type, and **Submit review**. Inline drafts survive interruptions but only recorded comments enter the handover. Overall feedback is included directly. Panel state, drafts, and highlights survive restart.

**Submit review** writes `review-feedback-<review-id>.md` without an AI pass or extra confirmation. If there is no recorded feedback and the overall field is blank, submission or `aide-review conclude` instead reports “Review concluded. No feedback recorded.” and removes the review state without creating a file. Exports contain **Overall feedback**, then **Changes** to implement, then **Questions** to answer and discuss or plan. Each inline comment retains its source reference or quoted guide passage. Wording and comment order within each group are preserved; empty groups are omitted. A completion message shows the export path, and the CLI exits. Existing files with different contents are never overwritten.

Walkthrough length should match the substantive change: small patches get short guides. Modified-file excerpts use complete overlapping hunks from the captured diff, retaining deletions and context. Block headers distinguish diffs, new files, and current code; clicking a block's title selects its source range for chat or feedback. A range in a mixed diff defaults to current lines; select deleted rows to reference the old side.

The harness owns its native session history; conclusion does not remove Codex history. Application actions, agent events, drafts, selections, and scroll positions are persisted locally until conclusion. Commands waiting for a connection are retained in browser storage and retried with stable IDs; the completed review's outbox is cleared. The UI only enables sending new agent work while connected.

## Tests

```sh
just test
just check
just setup-browser
just test-browser
just test-live
```

Automated tests cover source mappings, persistence, exports, VCS input, and browser workflows. The live check uses your configured Codex account and MCPs, and may incur normal model usage. Headless Playwright MCP is also used for interactive acceptance testing. See the [review plan](plans/REVIEW_PLAN.md) for scope and verification status.
