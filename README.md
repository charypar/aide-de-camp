# Aide de camp

> **Big honking warning**: This is entirely vibe coded, I haven't even read the code, and might not until I feel like it.
> It's very much built for my preferred tools and workflow – codex, tmux, Alacritty, Zed – therefore unlikely to work
> for yours.
>
> It's likely that the whole thing is quite rough, and will change a lot. Try at your own risk. You've been warned.

AI + Development Environment, and an aide-de-camp for development work. Aide contains two tools you can use together or independently:

- **Task management (`aide`):** create a workspace and Codex conversation for each task, switch between tasks, and clean up when finished.
- **Guided review (`aide-review`):** understand local changes through an AI-generated walkthrough, discuss the code, and collect feedback into a Markdown handover.

## Installation

Install Rust via [rustup](https://rustup.rs) and [Just](https://github.com/casey/just) (`brew install just` on macOS, or `cargo install just --locked`). Then, from this repository:

```sh
just setup
just install
```

This builds and installs both commands with their browser interfaces embedded. The installed tools do not need Node or Trunk at runtime.

Both tools use an already-installed, signed-in **Codex CLI** with your normal configuration and authentication. Review needs **Git or jj**. Task management additionally uses **Git, jj, tmux, and fish**, with **Alacritty** for dedicated terminal windows and **Zed** for editor access. Run `aide doctor` or `aide-review doctor` to check external commands.

## Task management

### Register a project and start the dashboard

A project is a directory containing one or more existing repository checkouts. Aide creates task workspaces beside each checkout:

```text
~/Projects/CatHerder/
├── cat-herder/                # Existing main checkout
├── cat-herder-tasks/
│   └── task-1234/             # Workspace created for a task
└── .aide/state.sqlite         # Project and task records
```

```sh
cd ~/Projects/CatHerder
aide init . --repo ./cat-herder
aide                          # Start the dashboard and open the browser
```

`init` registers an existing checkout; it does not create or clone a repository. For a Git-only checkout, it initializes colocated jj. Repeat initialization with another `--repo` to add a repository to the project.

The dashboard lists tasks across registered projects, with status and actions to open their terminal, editor, or review. Select a project to focus on it; **Show completed** includes its task history. Project registration is CLI-only.

Each task also shows **Terminal Idle / Working**, independently of Codex status and the tab's attention counts. This covers commands in all its tmux panes, including background jobs and servers while detached. Aide excludes its backend pane and managed Codex process trees; exiting Codex lets ordinary shell commands in that pane count again. `aide task list` includes the same activity. A dash means activity is unavailable. Detection observes processes: shell-only builtins and fully detached daemons may not be visible.

### Start, return to, and finish a task

Use **New task** in the dashboard, or run these commands from another terminal anywhere inside the project:

```sh
aide task start task-1234 --new-window
aide task list
aide task attach task-1234 --new-window
```

Starting fetches the repository, creates a colocated jj/Git workspace from `trunk()`, and opens a fresh Codex conversation for you to plan and execute the task. For projects with several repositories, add `--repo <name>`. Task IDs are unique within the project, including completed history.

The dashboard must be running to start or repair managed task sessions. For just a workspace, use `aide task start task-1234 --workspace-only`; this needs no dashboard and starts no agent.

`--new-window` opens a dedicated window in the existing Alacritty app using its IPC interface, keeping task windows together in the macOS Dock and app switcher. If no instance is reachable, Aide starts Alacritty. Without `--new-window`, start/attach uses the current terminal, switching sessions if you are already inside tmux. **Ctrl-b then d**, `aide task detach`, or closing the dedicated terminal window detaches and leaves the task running. Attaching to a healthy task works without the dashboard.

When ready, review the changes and publish your work manually. Then close the task from the dashboard or CLI:

```sh
aide task review task-1234     # Open guided review for this workspace
aide task cleanup task-1234    # Type the task ID to confirm deletion
aide task list --completed
```

Cleanup warns about live sessions, stops the entire task tmux session (including windows you added), and removes the workspace. Task history and retained conversation recordings remain available. Creating a PR or completing a review does not close the task automatically.

### Work inside the task terminal

Each task has two tmux windows:

- **work:** fish running the native Codex terminal UI. Exit Codex to return to fish, then run `aide codex resume` to reconnect in that terminal. Exiting the UI leaves its backend running.
- **backend:** the worker, dedicated Codex server, and recording monitor. Ctrl-C here stops the backend and interrupts active work.

With the usual tmux bindings, **Ctrl-b then w** selects a window and **Ctrl-b then c** creates one for a shell, jj commands, or a local server. You can rename or reorder windows. `aide codex resume` works from any window in the task's tmux session; from elsewhere, attach to the task first. Approvals and conversation stay in the native Codex UI.

To change the environment available to Codex tools:

1. Exit the Codex UI and stop the backend with Ctrl-C in its window.
2. Return to the work shell and change the exported environment variables.
3. With the dashboard running, run `aide codex resume`.

This starts a backend with that shell's environment and resumes the saved conversation idle. Resuming an already healthy backend only reconnects; it does not refresh its environment.

### Recovery and local data

Stopping the dashboard leaves tasks running. Starting it again reconnects to surviving sessions and restores missing sessions after a reboot. Restored conversations are idle and require attention; interrupted commands are not replayed. Conversations without saved history may reopen fresh. Aide restores its own windows, not arbitrary server commands you started yourself.

If a backend is stopped or its window deleted, use `aide codex resume`, `aide task attach <id>`, or the dashboard's **Recover session** action. Repair requires the dashboard and preserves user-added windows.

Project records live in `.aide/state.sqlite`; the personal project registry and retained native conversation recordings live under `~/.aide`. Use `--home` or `AIDE_HOME` to select another personal data directory. Codex retains its original sessions separately. A task stays associated with its saved conversation even if you start another conversation in the same directory.

`aide --no-open --port 4318` serves the dashboard on localhost without opening a browser. Independent task operations run concurrently; conflicting operations on one task report busy, while workspace changes sharing a repository wait their turn. Codex's app-server transport is experimental; this version was checked with Codex 0.163.0.

## Guided review

### Start with local changes

Run the reviewer in any Git or jj checkout; no Aide project registration or dashboard is needed:

```sh
cd /path/to/repository
aide-review
```

Equivalent entry points are `aide review` in the current repository and `aide task review <id>` for a registered task's workspace. Both forward the reviewer's options and subcommands.

The input is the whole repository's local change: staged, unstaged, and untracked changes against Git HEAD, or the jj working commit `@`. Ignored untracked files are excluded. To review an existing PR, first use Git or jj to prepare a workspace where its changes appear as that local diff.

Codex reads the repository and prepares a walkthrough explaining the implementation in a logical order, with text and code excerpts. The browser opens when the guide is ready, or earlier if preparation needs your input. The terminal prints the URL and progress immediately. The review agent uses a read-only sandbox; approval requests and questions appear in the browser.

### Understand the change and collect feedback

Read the walkthrough alongside your editor's full diff. The guide helps explain the approach; Zed remains useful for browsing surrounding code and inspecting every change.

- Select guide text or code, then choose **Discuss with agent** to ask a question with that reference attached.
- Choose **Add feedback** to record a comment at that location. Mark it as a **Change** or **Question**; discussion alone does not become feedback.
- Use **Reference a file range** in chat for code outside the walkthrough, including lines found in your editor. References support both old and new sides of the diff.
- Add broader notes in **Overall feedback** at the bottom of the page.

For code ranges, drag across rows or click a row then Shift-click another. Enter sends a chat message or records an inline comment; Shift+Enter inserts a newline. Comments stay editable, and drafts and reading position survive interruptions. Only recorded inline comments enter the handover.

**Submit review** writes `review-feedback-<review-id>.md` in the starting directory and exits. The file contains your overall feedback, changes to implement, and questions, retaining source references and your wording. There is no AI rewriting pass. Hand the file to an implementation agent or use it to prepare feedback for the author; nothing is submitted to GitHub automatically.

Submitting with no feedback concludes the review without creating a file. Successful conclusion removes the local review state; failed exports preserve it. Codex's native conversation history remains.

### Pause, resume, and review a new revision

Stop with Ctrl-C and run `aide-review` again from the same directory to resume. State lives in `.review/` in that directory. Add `.review/` and `review-feedback-*.md` to your repository's ignore file if desired.

Each session reviews a captured snapshot. Edits made while it runs do not update that snapshot. Stop and relaunch after changing code: the previous review is archived and a new guide is prepared. Feedback is not automatically transferred to the new revision. Archives remain in the local database until conclusion cleans it up.

Useful options and commands:

```sh
aide-review --path /path/to/repository
aide-review --no-open --port 4317
aide-review --port 0           # Choose a new port if the remembered one is occupied
# After stopping the running review server:
aide-review history           # List locally retained reviews
aide-review conclude          # Export saved feedback and clean up without the browser
```

If guide preparation fails or is interrupted, use **Retry preparation** in the browser. `conclude` exports the saved review even if the code has changed since the server stopped.

## Development and tests

Both tools belong to one Cargo workspace. `just --list` describes the available recipes.

```sh
just dev /path/to/repository   # Watch the review server and UI
just aide                     # Build and run the task dashboard
# For dashboard UI development, run these in separate terminals:
just aide-dev-ui
just aide-dev-server
```

The review development runner watches Rust and UI sources; refresh the browser after a UI build. Dashboard development watches UI assets; restart its server after Rust changes. `aide-review --fixture` uses a deterministic harness for UI development without model calls.

```sh
just fmt                      # Format Rust
just check                    # Formatting and Clippy checks
just test                     # Rust tests
just setup-browser            # Install optional browser-test dependencies
just test-browser             # Headless browser tests for both tools
just build                    # Build both tools with embedded browser assets
```

Live integration checks use your configured Codex account and MCPs: `just test-live` checks review, `just test-aide-protocol` checks task observation without a model prompt, and `just test-aide-live` checks native task conversations and recovery. `just test-aide-detached-status` checks that a working task becomes idle after detaching its terminal, using one short model turn. Checks that send model prompts may incur normal usage charges.

See the [coordination plan](plans/COORDINATION_PLAN.md) and [review plan](plans/REVIEW_PLAN.md) for scope, implementation decisions, and verification status.
