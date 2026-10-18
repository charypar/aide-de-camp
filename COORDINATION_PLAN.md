# DevDeck — task and workspace coordination plan

Product outline with initial scope agreed; milestones and implementation choices are next. The companion [review plan](REVIEW_PLAN.md) defines an independently usable tool.

**Repository:** the coordination workbench and review tool belong to the same DevDeck Cargo monorepo/workspace. They remain independently usable; exact crate boundaries and useful shared components will be established during implementation.

## Purpose and user

Minimize workspace housekeeping and time to regain context for a hands-on head of engineering who also contributes to open source. Work is frequently interrupted and shifts between coding, testing, and review. Slack and calls are outside scope.

Typically one main project has 10+ tasks; design for up to 10 projects and 30 tasks per project, with far fewer executing simultaneously. Every project has a repository; every task has a physical workspace and a contextual agent.

## Jobs and lifecycle

Primary jobs: start and finish tasks with minimal setup/teardown, see ongoing work, and enter the relevant workspace or conversation quickly.

1. Start a task directly in the dashboard or give the first officer a short intent: idea, ticket, task, or request. Conversational intake can clarify intent and prepare a brief; it is not required for direct creation.
2. Both entry points use the same setup: select the project, supply its private context, create a workspace from main, and start a fresh task-supervisor conversation. The first officer may delegate setup to the project supervisor.
3. Develop through whatever planning, plan review, and implementation the task needs. Instructions define this evolving workflow.
4. Prepare local changes for the independent review tool; receive its text feedback and hand it to the task supervisor.
5. Iterate, then open a PR for owned work or approve a teammate’s PR after review.
6. The user explicitly finishes the task, either by telling the first officer or closing it in the UI. Both routes trigger the same cleanup workflow. Opening a PR or an agent finishing its work does not automatically close the task.

Alternate entry: check out someone else’s work into a dedicated workspace with an attached contextual agent, then enter review.

## Dashboard and supervision

Create tasks and show all in-flight tasks across projects with waiting, working, and requires-attention statuses, including reasons. Each task offers access to Zed, a terminal session, its existing Codex conversation, and guided review.

**Shared visibility:** the user through the dashboard and the first officer through its tools have equivalent access to task status, workspace/session locations, and other dashboard information. Skills explain how the first officer discovers and inspects that information. Human and agent interfaces use the same underlying records and lifecycle operations.

Hierarchy: first officer → project supervisors → task supervisors → configurable role agents. This divides responsibilities; it does not require every request to traverse every level.

**First officer:** the usual entry point for new work. Clarifies intent, prepares briefs, and starts or delegates tasks. Coordinates overall staff, priorities, and execution shifts. Helps improve system efficiency by examining anomalies, suggesting improvements, and preparing briefings. The user remains the captain setting direction and judging outcomes.

**Project supervisor:** persists across tasks and maintains the user’s private, uncommitted project preferences, rules, and memory. Supplies this context to task supervisors and answers questions about project tasks, history, and status. Its focus is project continuity and overview; detailed implementation belongs to task supervisors. Private context is distinct from repository instructions, which focused agents must also respect.

**Task supervisor:** exists only for the scope of one task. Receives the brief and project context, refines the plan with the user, and initially does the implementation work itself. The user works with it directly but intermittently; execution should not require continuous conversation. End its active role when the user closes the task rather than reusing it for unrelated work. Retention of task records is a separate decision.

**Conversation lifecycle:** support suspending and resuming supervisor conversations with their existing context. Prefer controlling the harness’s lifecycle capabilities; the workbench maps sessions to roles/tasks and decides when work runs. Codex documents [resuming saved threads after process restart](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server). Conversation resumption and pausing in-flight tools/builds are separate concerns; validate execution behavior during technical design. Persistent project context does not require an always-running agent process.

**Later extension:** give task supervisors their own staff: one implementer and two reviewers providing independent opinions. The supervisor coordinates implementation → independent reviews → consolidated feedback → revision until reviewers are satisfied. Different reviewer models are an experiment to evaluate in practice, not a requirement.

Supervisors retain the agreed intent and decisions, track unresolved issues, and present results for human review. Delegation and reviewer-feedback consolidation arrive with agent teams. Behavioral policies—autonomy, clarifications, escalation, and loop stopping—belong in editable skills and will be refined later. Automated reviewer acceptance means ready for human review.

## Resource coordination

Support many open workspaces with only one or two executing at once across projects. Whole-workspace turns are the proposed starting policy; limiting only expensive operations is an alternative. Keep browsing/review available while execution queues.

Turn boundaries, prioritization, and subagent/build limits remain open. Rust compilation also creates disk pressure: artifact reclamation needs its own policy, preserving source changes and task context.

## Automations and system improvement

Schedule instructions for checks, reports, or other work through the task/workspace model. Repeated-run behavior remains open.

Improving the global system is a job in its own right. Global agent definitions and available skills are authoritative plain files, editable directly or through conversation. Proposed distinction: definitions describe roles/delegation; skills describe workflows; memory retains context and lessons.

Candidate recurring task: a weekly retrospective analyzes recorded conversations and proposes skill/memory updates with supporting examples, supporting the first officer’s improvement role. Recording, memory scope, and adoption of updates remain open. Focused agents must respect repository instructions; prefer the harness’s existing support, to be verified later. Private project-supervisor context must also reach each task, including tasks started directly by the first officer.

## Later: integrations and inbox

The dashboard should evolve into an inbox for incoming notifications and potential work, with information also accessible to supervisors. GitHub and Linear are examples; no provider set is fixed. Start integration exploration with MCP servers available to supervisors. How external events reach the dashboard and become tasks remains open. This inbox is a later iteration; task management comes first.

## Boundaries and preferences

- The workbench owns workspace preparation, task coordination, and subsequent PR/publication workflows. The review tool accepts a local Git/jj checkout with working-copy changes and returns coherent feedback tasks as text.
- Reuse Zed, jj, Alacritty + tmux + fish, and Codex. Provide human and MCP interfaces.
- Prefer local tooling, file configuration, and possibly SQLite, ideally without a persistent custom daemon.
- Start evolving automations as English-language skills; convert established workflows into software. Specialized UI and obvious deterministic automation can be built directly.
- Superset/Orca-style coordination is an experience reference, not a feature commitment.

## Initial scope and next steps

Task management first: support direct dashboard creation and conversational creation, with task supervisors working solo. Keep the first officer and dashboard equally informed, and make explicit user closure the trigger for cleanup. Add task-supervisor teams and the integration-backed inbox later. Detailed behavioral policies are skill-writing work, not prerequisites for milestone planning.

Next: agree milestones around a complete create → work → inspect/resume → close journey and observable acceptance criteria.

During technical design: dashboard form; execution scheduling and disk budgets; conversation attachment and recovery; cleanup mechanics and retained records; recurring-run behavior; definition/memory updates; preparation of imported PRs for local-diff review. Detached tmux sessions are a candidate runtime primitive, not a decision.

Planning path: product outline → milestones → implementation outline → technical design and executable plan. Discuss options, recommend, agree, then record. Proposed success signals: less repository housekeeping, faster task resumption, and manageable resource use.
