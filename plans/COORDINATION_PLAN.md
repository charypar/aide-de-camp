# Aide de camp — task and workspace coordination plan

Milestone 1 is implemented: workspace management, native attachable Codex sessions, monitoring/recovery, recordings, dashboard, and review launch integration. Milestone 2 remains open for experience with this version. The companion [review plan](REVIEW_PLAN.md) defines an independently usable tool.

**Naming:** Aide de camp in full, Aide for short, `aide` for the CLI. Keep `~/.aide` and project-local `.aide/` paths.

**Review commands — milestone 1:** rename the independent review executable to `aide-review`. `aide review` delegates to it for the current repository, without requiring a registered project or running dashboard. `aide task review <task-id>` resolves the task's workspace from project bookkeeping and launches the same review tool there; the dashboard's review action uses this operation. Preserve the review tool's arguments and normal start/resume behaviour. This integration only locates and launches the reviewer: it neither completes the task nor automatically submits feedback to its Codex conversation. All three command entry points are implemented.

**Repository:** the coordination workbench and review tool belong to the same Aide Cargo monorepo/workspace. They remain independently usable; exact crate boundaries and useful shared components will be established during implementation.

## Purpose and user

Minimize workspace housekeeping and time to regain context for a hands-on head of engineering who also contributes to open source. Work is frequently interrupted and shifts between coding, testing, and review. Slack and calls are outside scope.

Typically one main project has 10+ tasks; design for up to 10 projects and 30 tasks per project, with far fewer executing simultaneously. A project is a directory containing one or more related repository checkouts and private project files. **One task, one physical workspace; closing the task tears down that workspace.** Initially the user can launch an agent manually; managed harness sessions follow within milestone 1.

## Jobs and lifecycle

The full workflow below is the product direction; milestone 1 starts with direct workspace operations before first-officer/MCP integration.

Primary jobs: start and finish tasks with minimal setup/teardown, see ongoing work, and enter the relevant workspace or conversation quickly.

1. Start a task directly in the dashboard or plan it with the project supervisor; the first officer can route an initial request. The project supervisor draws on project history, personal context, and available tracker integrations. Conversational intake is not required for direct creation.
2. In milestone 1, select the project/repository and task ID, create a workspace from the latest trunk as resolved by jj, and open a fresh interactive Codex conversation in its terminal. The user plans and executes the task there; no initial brief or automatic implementation turn is required. Later, the project supervisor can supply a plan, launch the task or implementation loop, and bring it back for human review. During milestone 1, the user plays the project-supervisor role.
3. Develop through whatever planning, plan review, and implementation the task needs. Instructions define this evolving workflow.
4. Prepare local changes for the independent review tool; receive its text feedback and hand it to the task supervisor.
5. Iterate, then open a PR for owned work or approve a teammate’s PR after review.
6. The user explicitly finishes the task, either by telling the first officer or closing it in the UI. Both routes trigger the same cleanup workflow. Opening a PR or an agent finishing its work does not automatically close the task.

Alternate entry: check out someone else’s work into a dedicated workspace with an attached contextual agent, then enter review.

## Dashboard and supervision

**Manual parking — 2026-10-30:** expanded Ready tasks offer a secondary “Park task” action. Parked tasks appear in a muted group below active work and above completed history, excluded from Ready and attention counts. An explicit “Unpark task” action returns them to Ready. Expanding/collapsing details and copying paths are passive. Opening Terminal, Zed or Review, explicit session recovery/resume, or observing Codex Working/Requires attention clears parking permanently. Background terminal activity alone does not. Only Ready tasks may be parked, so active work and errors stay visible. Parking is independent of runtime intent and does not stop sessions. Store it outside task JSON to survive reloads, manager restarts and writes from older workers. CLI park/unpark and task listing share the same behavior. No timers, external dependency integrations or required notes in this iteration.

Parking validation: all 24 automated Aide Rust tests and all 22 dashboard browser cases passed. Two overlapping browser runs collided on trace output; the affected layout and parking edge-case tests passed with separate output directories. Coverage includes persistence across manager restart and older worker writes, passive inspection, explicit/automatic unpark, CLI parity, background commands, failed workspace actions, backend failure/recovery and mobile bounds. Headless Playwright MCP independently verified parking, reload, copying a path, manual unpark, terminal return, tab counts and mobile layout. Native Codex idle observation and seven configured MCP entries passed without a model prompt; actual Zed/Review GUI launches used fixture executables, and review selection/chat/feedback flows were unchanged and not rerun. Dashboard/server builds, formatting, and server/UI Clippy passed.

The browser-tab title shows `(N·M) Aide de camp`, counting non-completed tasks in Requires attention (first) and Ready (second) across all projects, including when viewing a single project. With both counts zero, use `Aide de camp`. Update with the normal status snapshots. Browser notifications are a later enhancement: opt-in, triggered by status transitions rather than every poll; delivery with the dashboard closed needs a separate design.

**Terminal activity — implemented:** a separate neutral status tag beside the Codex status shows aggregate Terminal Idle / Working across all task panes, including while detached; `task list` reports it too. Keep the Open terminal button as a separate action, with compact terminal icons on narrow screens. External commands, background jobs and servers count as Working. Exclude the backend by stable pane ID and managed Codex process trees by PID plus process start time, so renamed/reordered windows and commands after UI exit work correctly. This is independent of agent status and attention/ready counts. Inspect tmux/process metadata once per dashboard snapshot; unavailable inspection or absent sessions show a dash rather than Idle. Shell-only builtins and fully detached daemons remain outside reliable detection. Per-window details are deferred.

Terminal activity validation: Rust tests cover process identity, legacy native UI detection, dead panes, missing metadata and PID reuse. Browser fixtures cover detached multi-pane activity, background jobs, renamed/reordered windows, Codex descendants, UI exit, reload and mobile layout. Headless Playwright MCP verified the live indicator, unchanged attention counts and terminal action. `just test-aide-terminal-activity` verified installed Codex 0.164.0 exclusion, shell activity and resume without model prompts.

**Dashboard refinement — 2026-10-26:** use the review tool's cool grey palette, white header and surfaces, blue accents, typography, and control styling. The full task title area (including repository and surrounding space) toggles details, with keyboard activation and a visible focus indicator; terminal actions remain separate. New-task forms select the first available project repository by default and preserve an explicit selection while the form is reopened or task snapshots refresh. Keep controls and expanded details usable on narrow screens. Verified with all 9 dashboard browser tests, headless Playwright MCP desktop/mobile checks, and formatting/Clippy checks. Harness integration is unchanged; native Codex checks were not rerun.

**Task form and notifications — 2026-10-28:** “Start and open Codex” closes its form once the request is sent; a send failure leaves it open. Reopening retains the task ID and repository for retries. Dashboard operation messages appear as top-right toasts for five seconds, with a border and thicker left bar: blue for progress, green for success, red for errors. Each replacement, including an identical message, restarts the timer; status snapshots do not extend it. Toasts fit narrow screens and do not shift page content. Verified with all 15 dashboard browser tests, formatting and UI Clippy, plus headless Playwright MCP task creation, success/error expiry, mobile bounds and reload. Native Codex integration is unchanged and was not rerun.

Keep “Show completed” beside “All projects” on the left of the navigation row, leaving the right side for toasts. Verified with two dashboard regression tests and headless Playwright MCP alignment and checkbox interaction checks at desktop and mobile widths.

List active tasks grouped first by project, then by status (waiting, working, requires attention), with reasons. Navigate from the overview to a focused single-project listing; provide a show/hide completed toggle for project history. Collapsed tasks occupy one line: the native Codex summary is the primary title ("Untitled task" until available), the task ID is a compact label, and a text-labelled status badge distinguishes attention (prominent amber), waiting/ready (blue), working (subdued grey), and completed (muted grey). Colour and group order prioritize the user’s attention: requires attention first, then ready, working, and completed. Display the internal waiting state as "Ready" in the dashboard. Keep terminal access separate from the expansion toggle. Truncate long summaries on the row; expansion reveals the full summary, repository, status reason, paths, session details, and cleanup. Preserve the compact row on narrow screens with an accessible terminal icon. Verified with the dashboard browser suite and headless Playwright MCP at desktop and mobile widths.

Detached status investigation — 2026-10-26: a reported stale working indicator after closing a terminal could not be reproduced. Read-only checks found the user's backend and dashboard both idle. The repeatable `just test-aide-detached-status` check attaches a real terminal client, detaches during a native Codex turn, and verifies Working → Waiting without reattaching or exiting the Codex UI. It passes with Codex 0.164.0. No monitoring behavior was changed; the user subsequently confirmed updates were working and suspected observing the normal polling delay.

Task creation is available from both overview and project views; a button in each project heading is the preferred starting idea. Project initialization stays CLI-only in milestone 1. An empty dashboard should explain `aide init . --repo ./project`, and newly registered projects should become visible without recreating the dashboard. Each task offers access to Zed, its native harness terminal, and guided review.

**Starting and returning — milestone 1:** starting opens the prepared workspace's fresh Codex conversation for interactive planning and execution. Dashboard start opens a dedicated terminal window; CLI start uses the current terminal's attach/switch behaviour. Returning simply attaches to the existing conversation. If Codex exposes a useful idle conversation summary through an easy supported read path, show it as an optional convenience; availability is unverified and it is not a delivery requirement. Do not add a separate summarisation agent or require a brief. Brief-driven background starts can follow later.

**Startup visibility — 2026-10-29:** show a neutral Starting state from workspace preparation through runtime readiness, excluded from both attention and ready counts, with runtime actions hidden. Keep one task lifecycle lock across managed workspace creation and backend launch. Store pending startup separately from task JSON so it survives dashboard reloads and writes from older workers. Failed or interrupted startup requires attention with a stage-appropriate retry reason; workspace preparation retries use the same task ID, while runtime failures use session recovery. Recheck lifecycle after backend observation to avoid publishing stale startup failures. Workspace-only starts become ready when preparation finishes; stopped existing backends still require attention.

Startup validation: 21 automated Rust tests and all 19 dashboard browser tests pass, including delayed preparation/backend startup, no attention snapshots during successful creation, reload, failed preparation and backend launch, manager termination/restart and recovery, and existing closing/concurrency behavior. Headless Playwright MCP independently confirmed Starting, excluded counts, hidden runtime actions, reload and mobile bounds; the follow-up Ready/cleanup check was cancelled, and those transitions were covered by the automated suite. Native Codex idle observation/configuration passed with seven MCP entries and no model prompt. Formatting and server/UI Clippy checks passed.

**Closing — milestone 1:** the dashboard's Close task action and CLI `aide task cleanup <task-id>` require explicit confirmation before stopping task runtimes and removing the workspace. Warn when an agent is working or a Codex session is still open, distinguishing active execution from an idle session where known. Closing completes the task and retains its history/recordings. Detaching or closing a terminal only leaves the task running; a separate suspend/stop-without-completing control is not needed initially.

During cleanup, retain the available conversation title in project metadata and show a neutral Closing state, excluded from attention counts, with runtime actions hidden. Recheck lifecycle after observing the backend so its expected shutdown cannot produce a misleading attention state. A closing task without an active lifecycle lock requires attention and offers cleanup retry. Retain the title in completed history; separate metadata protects it from surviving workers from older installations. Regression coverage includes delayed cleanup, reload, interrupted cleanup/retry and durable title retention.

**CLI and browser:** task-management commands work from inside a project, including repository/task subdirectories, and discover the enclosing project root. Running `aide` with no arguments explicitly starts the local manager and browser dashboard covering all registered projects. Managed runtime creation/recovery uses the manager API; commands do not silently start a background manager. Attaching to an existing tmux session only needs the recorded session identity and works without the manager. Project initialization and first-slice workspace operations can run locally.

**Manager lifecycle:** the dashboard manages and observes task runtimes, but does not own their lifetime. Closing a browser, task terminal, or Aide itself must leave task tmux sessions and Codex backends running. On dashboard startup, reconcile persisted runtime intent with reality: reconnect to surviving components and recover missing runtimes for previously running tasks, including after a machine reboot. Store intended runtime state separately from observed process status so deliberately stopped tasks remain stopped. Manage only Aide’s task sessions, never the shared tmux server or unrelated sessions.

**Shared visibility:** the user through the dashboard and the first officer through its tools have equivalent access to task status, workspace/session locations, and other dashboard information. Skills explain how the first officer discovers and inspects that information. Human and agent interfaces use the same underlying records and lifecycle operations.

Hierarchy: first officer → project supervisors → task supervisors → configurable role agents. This divides responsibilities; it does not require every request to traverse every level.

**First officer:** the usual entry point for new work. Clarifies intent, prepares briefs, and starts or delegates tasks. Coordinates overall staff, priorities, and execution shifts. Helps improve system efficiency by examining anomalies, suggesting improvements, and preparing briefings. The user remains the captain setting direction and judging outcomes.

**Project supervisor:** persists across tasks and maintains the user’s personal preferences, rules, and memory for that project. Plans tasks with the user using project context, active/completed task records, and tracker information; launches the implementation with the plan, supervises progress, and requests human review. Detailed implementation belongs to task agents. Personal context is distinct from shared project conventions, which focused agents must also respect. This is the future workflow; the user initially fills this role.

**Task supervisor:** exists only for the scope of one task. Receives the brief and project context, refines task details with the user as needed, and initially does the implementation work itself. The user works with it directly but intermittently; execution should not require continuous conversation. End its active role when the user closes the task rather than reusing it for unrelated work. Keep the completed task’s bookkeeping in the project database after workspace cleanup.

**Conversation lifecycle:** support suspending and resuming supervisor conversations with their existing context. Prefer controlling the harness’s lifecycle capabilities; the workbench maps sessions to roles/tasks and decides when work runs. Codex documents [resuming saved threads after process restart](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server). Conversation resumption and pausing in-flight tools/builds are separate concerns; validate execution behavior during technical design. Persistent project context does not require an always-running agent process.

**Durability:** accidental tmux/harness termination or a machine reboot must not end the task. Dashboard mode recovers missing tmux sessions and, where supported, resumes the stored native conversation in the existing workspace, both on startup and through a recovery action. Recovered conversations remain idle and tasks show requires attention with a recovery reason; do not submit another agent turn or replay interrupted commands automatically. Reconnecting to a surviving runtime leaves its ongoing work alone. `task attach` switches to an existing session directly; if it is missing, request recovery from the manager, or explain that dashboard mode must be started. Prevent duplicate runtimes. Explicitly completed tasks are not recreated.

**Session recording — milestone 1:** retain Codex’s native session recording format for later analysis; do not invent a separate transcript schema. Store the native session ID and references to retained recording artifacts under the global home in the project database. Preserve available conversation and tool/lifecycle history, not just terminal output or current status. Records must survive workspace cleanup and dashboard closure, including harness restarts. Validate native artifact capture/retention without disrupting the user’s Codex configuration or authentication. Retrospective analysis itself remains future work.

**Later extension:** give task supervisors their own staff: one implementer and two reviewers providing independent opinions. The supervisor coordinates implementation → independent reviews → consolidated feedback → revision until reviewers are satisfied. Different reviewer models are an experiment to evaluate in practice, not a requirement.

Supervisors retain the agreed intent and decisions, track unresolved issues, and present results for human review. Delegation and reviewer-feedback consolidation arrive with agent teams. Behavioral policies—autonomy, clarifications, escalation, and loop stopping—belong in editable skills and will be refined later. Automated reviewer acceptance means ready for human review.

## Resource coordination

Support many open workspaces with only one or two executing at once across projects. Whole-workspace turns are the proposed starting policy; limiting only expensive operations is an alternative. Keep browsing/review available while execution queues.

Turn boundaries, prioritization, and subagent/build limits remain open. Rust compilation also creates disk pressure: artifact reclamation needs its own policy, preserving source changes and task context.

## Automations and system improvement

Schedule instructions for checks, reports, or other work through the task/workspace model. Repeated-run behavior remains open.

Improving the global system is a job in its own right. Agent definitions and skills are authoritative plain files, editable directly or through conversation. Proposed distinction: definitions describe roles/delegation; skills describe workflows; memory retains context and lessons. Do not store these editable definitions in the operational database.

Candidate recurring task: a weekly retrospective analyzes recorded conversations and proposes skill/memory updates with supporting examples, supporting the first officer’s improvement role. Begin recording in milestone 1; memory scope and adoption of updates remain later decisions. Focused agents must respect repository instructions; prefer the harness’s existing support, to be verified later. Private project-supervisor context must also reach each task, including tasks started directly by the first officer.

## Later: integrations and inbox

The dashboard should evolve into an inbox for incoming notifications and potential work, with information also accessible to supervisors. GitHub and Linear are examples; no provider set is fixed. Start integration exploration with MCP servers available to supervisors. How external events reach the dashboard and become tasks remains open. This inbox is a later iteration; task management comes first.

Future project supervisors use project-appropriate integrations and authenticated access, initially envisaged for GitHub and Linear, through MCP servers. Custom integration or credential management is not required for milestone 1.

## Boundaries and preferences

- The workbench owns workspace preparation, task coordination, and subsequent PR/publication workflows. The review tool accepts a local Git/jj checkout with working-copy changes and returns coherent feedback tasks as text.
- Reuse Zed, jj, Alacritty + tmux + fish, and Codex. Provide human and MCP interfaces.
- Prefer local tooling and file configuration, with SQLite for most project operational bookkeeping, ideally without a persistent custom daemon.
- Start evolving automations as English-language skills; convert established workflows into software. Specialized UI and obvious deterministic automation can be built directly.
- Superset/Orca-style coordination is an experience reference, not a feature commitment.

## Personal and shared configuration

- **Shared project configuration** expresses an agreed way of working with other contributors. Shared instructions, skills, and agent definitions belong in the relevant repository.
- **Personal configuration** expresses how the user prefers to work and governs most of this workbench’s workflow. It can apply globally or to a particular project; project-scoped personal preferences are still personal, not team conventions.
- **Global home:** `~/.aide`, including personal agent definitions, skills, and configuration. Personal project settings may be independently version-controlled outside the shared repository.

Initial adoption is individual, with no requirement for teammates to use the workbench or change shared repositories. Allow useful personal practices to migrate gradually into shared conventions when agreed. Personal workflow choices coexist with shared repository requirements; exact loading/precedence rules and configuration formats are later design work.

## Milestone 1 — workspace lifecycle and attachable harness

Two delivery slices within one milestone:

1. **Workspace management alone:** implement ordinary software for creating, listing/locating, and explicitly closing task workspaces. Each workspace has a stable path and is ready for the user to enter and launch the harness manually. Closing a task tears down its workspace. This is the smallest useful slice.
2. **Managed harness sessions:** start, monitor, and record a task’s harness; let the user attach to its native interactive session, converse, detach, and return. Preserve the task/session association and make exited/interrupted states clear. Task supervisors do the work themselves; agent teams come later.

The complete milestone includes both slices. Proposed acceptance journey: create two task workspaces, work independently in them, launch and return to each managed harness session, then explicitly close a task and clean up its workspace without affecting the other.

Terminal acceptance: open a task in its own Alacritty window, close that window (including Cmd-W), and reattach to the same live harness conversation. Closing a terminal must not terminate the task or trigger cleanup. Verify native tmux detachment as well.

Shell acceptance for the runtime revision below: exiting the Codex UI leaves the backend and active work running. Resume reconnects without restarting a healthy backend. To repair its environment, stop the backend, change exported variables in the work shell, and run `aide codex resume`; verify the recreated backend's commands see additions, changes, and removals from that shell's exported environment. The tmux session and task remain open throughout. No separate restart command is needed.

Recovery acceptance: stop or kill Aide and verify the same task sessions/backends keep running and remain attachable without it; restart the dashboard and reconnect without duplicates. Separately terminate tmux or a task backend and recover through the dashboard or `task attach`. Test reboot-equivalent loss of all runtime processes: dashboard startup restores previously running tasks and their saved conversations, leaving deliberately stopped/completed tasks alone. Verify preserved workspace changes, clear interruption status, durable recordings, and no effects on unrelated sessions.

Dashboard/recording acceptance: create a task from either overview or project view, expand it and enter its tools, then clean it up and find its completed record through the history toggle. Verify the linked session recording remains available after dashboard restart and workspace removal, including conversation activity while the dashboard was closed.

Start/close acceptance: starting a task opens a fresh Codex conversation in the trunk-based workspace without submitting a task prompt. Returning attaches to the same conversation. Both dashboard and CLI closure require confirmation, show live-session warnings, and leave everything untouched on cancellation. Confirmed closure stops only that task's runtimes and cleans up its workspace while preserving its history and recordings.

The user prefers native harness interaction over a recreated/proxied chat interface. Use tmux for terminal attachment; the harness connection and monitoring need live validation. MCP access for the first officer is a later pass over the same workspace operations. The full supervisor hierarchy, agent teams, scheduled work, integration inbox, and richer resource scheduling are later capabilities; milestone 2 stays open until experience with milestone 1.

## Project directory layout

A project has one root and N repositories. Each repository has a main checkout and a task-workspace folder. Operational bookkeeping lives in a dot folder at the project root, mostly in SQLite. Illustrative names:

```text
~/Projects/CatHerder/
  .aide/
    state.sqlite              # Project operational bookkeeping
  cat-herder/                  # Main repository checkout
  cat-herder-tasks/
    <task-id>/                # Dedicated task workspace
  another-repo/               # Optional related repository
  another-repo-tasks/
    <task-id>/
  settings/                   # Possible private preferences repo; not specified yet
```

Task IDs can be supplied at creation; later integrations may derive them from ticket or PR numbers. The project container leaves room for other files and independently version-controlled personal preferences. Do not introduce a settings schema or require a settings directory yet.

Operational state includes project/repository registration, task/workspace records, lifecycle progress, and eventual harness-session associations. Derive live facts from the filesystem, VCS, and harness where possible; these remain authoritative for their own state. The project database sits outside disposable workspaces. Exact schema, dot-folder/database names, and retention remain implementation decisions. Private project preferences may have a separate editable/version-controlled home; their format is not being specified yet.

**Global/project split:** keep a small SQLite database under `~/.aide` for registered project identities/paths and genuinely global operational state. Each project’s `.aide/state.sqlite` owns its repositories, tasks, workspace lifecycle, and harness-session associations. The dashboard aggregates those project databases; do not duplicate authoritative task records globally. Unavailable projects remain visible as unavailable. Global configuration, skills, and agent definitions remain editable files.

**Discovery:** the project’s `.aide/state.sqlite` is itself the marker; no separate marker file. Commands search ancestors for it and validate it as a project database. The global registry lets the dashboard find projects without scanning the disk. A project remains locally operable without its global registration, and can be re-registered after moving.

## Workspace commands — first slice

- **Initialize:** `aide init . --repo ./project` initializes the current project root, registers the existing repository, and registers the project globally. Multiple repositories remain supported; exact subsequent registration syntax can be settled during implementation.
- **Start:** `aide task start task-1234`, with a repository selector when needed. Prefer colocated jj/Git for both the main checkout and task workspaces. Refresh remote references, resolve the repository’s configured `trunk()`, and create a fresh working-copy change on that revision in `<repo>-tasks/<task-id>`. Do not assume a branch named `main` or inherit the main checkout’s in-progress work.
- **Publish manually:** the user works in the task directory and handles pushes, PRs, and other delivery actions themselves in this slice.
- **Clean up:** `aide task cleanup task-1234` asks for confirmation, warning about any working agent or open Codex session. After confirmation, stop that task's runtimes, remove its managed workspace and VCS workspace registration, and mark the retained task record completed. Preserve history and recordings. It does not publish, merge, or require PR-state checks. The user decides when it is disposable; cleanup is not triggered by apparent agent completion. Cancelling leaves the task and runtime untouched.

Second-slice commands: `aide task attach task-1234` enters its native harness session, recovering a lost runtime with the stored conversation when necessary; `aide task detach` leaves it running. Detach uses native tmux client detachment; normal tmux session switching remains available. Keep native tmux navigation available while Codex owns the foreground terminal; the shell command need not be typed into Codex chat.

Preferred dashboard interaction: open a dedicated Alacritty window for the selected task on demand. Proposed CLI option: `task attach --new-window`; ordinary attach uses the current terminal. Closing the dedicated window disconnects that client while the tmux session and harness continue. Open windows only when requested.

Window launch refinement — 2026-10-26: use `alacritty msg create-window` to keep task windows in the existing app process, including macOS Dock/app-switcher grouping. Retry discovery without an inherited socket if it is stale, then launch Alacritty only if IPC is unavailable. Preserve title, working directory, and the explicit task tmux target. Resolve attachment executables before sending the request because a Dock-launched instance may have a different PATH, and clear inherited tmux client variables inside the launched command as well as in the requesting process. Regression coverage checks IPC reuse, stale-socket discovery, fallback, and exact attachment arguments. A live Alacritty 0.15.1 check confirmed two windows reused the existing process and detachment preserved the task. Cmd-W itself was not simulated. The optional native acceptance script now exercises Aide's launcher and detaches only its test clients rather than terminating the shared Alacritty process.

Proposed failure behavior: if refreshing trunk fails or `trunk()` does not identify a usable base, report the problem rather than silently create from stale history or jj’s empty-root fallback. Keep task creation/removal recoverable if filesystem, VCS, or database steps fail. Detailed recovery mechanics are implementation work.

Local inspection: installed jj 0.46.0 exposes `workspace add --colocate` and explicit parent revision selection, matching the [jj release notes](https://github.com/jj-vcs/jj/discussions/10339). No workspaces were created or removed during planning.

## Technology direction

Reuse the review tool’s stack in the same Cargo workspace: Rust, Clap CLI, Tokio/Axum local server, Leptos CSR with Trunk, SQLite through rusqlite, and WebSocket updates to the browser. Follow its embedded-asset distribution and root Just recipes. Share reusable components where justified while keeping product state separate; do not couple the workbench to review-specific session lifetimes or chat UI. The harness remains an external dependency.

## Runtime revision — implemented

Use two managed windows in each task's peer tmux session: a work window containing fish and the native Codex UI, and a backend window containing the worker and dedicated Codex server. Exiting the UI must leave the backend and active work running. Tmux owns both windows; the worker handles recording and backend shutdown. Users can freely add windows for jj, local servers, and other work. Identify managed windows by stable tmux IDs/metadata, not their position or user-editable names; do not replace user-added windows during runtime recovery. Explicitly closing the whole task ends its whole session, including those windows. Reboot recovery restores managed windows, not arbitrary commands from user-added windows.

Recovery: retain the backend pane after its worker exits (`remain-on-exit`) so its output and stopped state remain visible. Stopping that worker stops its backend and interrupts active agent work, but does not complete the task. Attach/resume should check session/window identity and backend responsiveness, reuse healthy components, and repair missing or stopped managed components through the existing recovery path. Resume saved context idle and requiring attention; do not replay interrupted turns or run an immediate restart loop that fights a deliberate stop. Preserve user-added windows and never replace a pane running an unrelated command. Report failed repairs clearly and do not resurrect closing/completed tasks.

First-class control namespace: `aide codex resume`, replacing the shell-only `aide-resume` helper as the documented entry point. Use the current registered task tmux session as the task context, and open the native UI in the terminal where the command runs. This works from user-added windows and after changing directories. If the working directory belongs to a different task, report the mismatch rather than connecting silently. Outside a task session, direct the user to `aide task attach <id>` first; omit the proposed `--task` selector on resume. Do not infer the last-used task. Resume reconnects to a healthy backend without restarting it or submitting a turn, and works without the dashboard when the backend is alive. Missing-runtime recovery follows the existing manager policy. When resume recreates a stopped backend, use the invoking shell's exported environment, including variable removals, rather than the dashboard or tmux server's stale environment. Stopping the backend and then resuming is the environment-repair workflow; no separate restart command is needed. Additional harness namespaces can follow without implementing a general harness abstraction now.

Conversation association: after initial discovery of exactly one matching native conversation with saved history, bind its ID in the project database. That ID is authoritative for monitoring and resume; other conversations in the same directory cannot replace it. Ambiguous initial discovery requires attention. Untouched conversations without saved history remain unbound and can reopen fresh.

Codex 0.165 compatibility: initial discovery considers user conversations only. Loaded `thread_title` and other internal threads may share the workspace and client source but must not count as competing conversations. Use `threadSource` when available, with legacy subagent-source filtering for older servers. Successful observation/recording clears stale monitoring errors without clearing unrelated task errors. Verified with native metadata from two affected tasks, browser regression coverage for helper exclusion/error recovery and genuine ambiguity, and live dashboard checks after repairing their missing associations without restarting agents.

Recording efficiency: inspect native recording metadata at most every ten seconds and copy only changed files; flush pending changes on orderly backend shutdown and task cleanup. The cache is in memory, so a restarted worker may make one fresh copy. Native recordings remain the source of truth.

Concurrency: independent management commands run concurrently, including commands from one dashboard connection. Project-local task locks protect creation, recovery, attachment and cleanup across processes; conflicting task transitions report busy and can be retried. Repository mutations wait on a lock in Git’s common directory, shared by related checkouts. Short SQLite transactions protect read-modify-write updates; no project-wide lock spans a fetch or workspace deletion. Repository waits and deletion run on blocking workers. Startup recovery runs alongside the dashboard, and accepted operations continue if the browser disconnects. These locks coordinate Aide operations; jj/Git retain responsibility for external commands.

The root `README.md` now provides setup and usage instructions, a copyable first trial, tmux navigation, environment repair, recovery, review, and confirmed cleanup. It explains manager requirements and retained history/recordings.

## Implementation and validation — initial version

- Separate `aide-core`, `aide-server`, and Leptos `aide-ui` crates share the Cargo workspace with the independently usable reviewer. `just install` builds and installs `aide` and `aide-review` with embedded assets.
- Project SQLite records are authoritative; the personal registry tracks project roots. Task IDs cannot be reused after completion. Operation locks, workspace ownership markers, and explicit preparing/closing states support safe retries.
- The installed initial version uses a named peer tmux session with one fish pane. A task-local wrapper owns a separate Codex app-server on a private Unix socket and its native terminal client. Dashboard shutdown does not own or terminate those processes. Exiting Codex returns to fish; `aide-resume` refreshes the backend environment and resumes the conversation. This coupling is superseded by the implemented runtime revision above.
- Monitoring uses metadata-only `thread/read`, without subscribing to or responding to approvals. Native thread names are optional labels; no separate summary generation. Native recording files are copied under the personal home by the task wrapper even while the dashboard is stopped.
- Codex may not persist an untouched empty conversation. Recovery resumes a durable conversation by ID; a never-used blank session can reopen fresh. Recovered tasks require attention and no new agent turn is submitted. Recovery is available on dashboard startup, through its action, and when attaching to a missing terminal. A hidden `task recover` command exists for testing.
- Automated coverage includes jj workspace isolation, project discovery, failed fetches, interrupted cleanup, ownership protection, native status mapping, recording retention, dashboard creation/status/history, manager loss, session restoration, confirmation/cancellation, and review command forwarding. Review regression tests cover selection, chat, feedback, and restoration.
- Live checks use installed Codex 0.163.0 and native configuration/MCP discovery. They cover independent observation, a persisted conversation, fish environment repair reaching the resumed backend, Alacritty attachment/window closure, and task survival across dashboard shutdown. The opt-in `just test-aide-live` reproduces the native conversation/recovery journey; `--window` on its Python script includes an actual Alacritty window. App-server transport remains experimental.

Remaining work is iteration on the first version. Supervisor teams, scheduling, resource arbitration, integrations, and automatic feedback handover remain later capabilities.

Verification: 40 automated Rust tests and 28 headless browser tests pass, along with formatting, Clippy, release builds, and local installation. Live native checks confirmed saved conversation restoration and environment repair; headless Playwright MCP independently exercised dashboard confirmation/history and the installed task-review shortcut's selection, chat, feedback, reload, and export. Alacritty window closure was tested by terminating the dedicated test window process; the Cmd-W shortcut itself was not simulated.

Runtime revision validation: fixture acceptance covers independent UI/backend lifetime, reconnecting without the dashboard, exported environment additions/changes/removals, task-session context and mismatched directories, stopped/deleted backend recovery, preserved extra windows, and refusing to replace unrelated commands. The native Codex check confirmed an active turn completed after UI exit, healthy reconnect reused the worker, environment repair reached agent tools, and reboot-equivalent recovery restored the same conversation idle. Headless Playwright MCP exercised task creation, backend-stop recovery, browser reload, cancelled/confirmed cleanup, and retained history. Concurrent recording copies now use independent temporary files. Revision checks passed: 8 Aide Rust tests, 5 browser/runtime acceptance tests, formatting/Clippy, release build and local installation. Deliberately stopped backends in surviving sessions await explicit attach/resume/recovery; dashboard startup automatically restores missing whole sessions. Native protocol transport remains experimental.

Conversation/recording follow-up verified: 10 Aide Rust tests and 7 browser/runtime tests pass, including stable persisted binding, ambiguous initial discovery, changed-file detection, coalescing and shutdown flush. Live Codex recovery and retained recordings passed; headless Playwright MCP confirmed the dashboard keeps the bound ID when another same-directory conversation appears. Formatting and Clippy passed.

0.1 concurrency validation: 12 Aide Rust tests pass, including independent task locks and concurrent SQLite updates. Browser acceptance covers a deliberately blocked fetch, another repository progressing on the same dashboard connection, shared-repository serialization, duplicate CLI creation, and attach-versus-cleanup exclusion. Headless Playwright MCP confirmed independent progress and reload; native Codex restored the saved test conversation idle under the new locks. Remaining hardening: isolate monitoring failures from backend lifetime and strengthen recovery after forcibly killing a worker.
