# Guided review — product plan

Product plan and implementation handover for the first working milestone. Rust/Axum, Leptos CSR, SQLite, WebSocket application communication, and native Codex app-server integration are implemented. Milestone 2 remains open for reconsideration after using milestone 1. The implementation progress section records current decisions and verification; protocol comparisons below retain the planning research. Works independently of the companion [coordination workbench](COORDINATION_PLAN.md).

**Repository:** this tool belongs to the DevDeck Cargo monorepo alongside the coordination workbench described in `COORDINATION_PLAN.md`. Both tools live in one Cargo workspace and remain independently usable. Exact crate boundaries are an implementation detail; agent integration and other common code can be shared where useful.

## Purpose and user

Help a frequently interrupted engineering lead understand and judge a change quickly. Both human- and agent-authored work require assessing the technical approach; human-authored work generally needs more intent and context discovery.

## Input, agent, and output

**Input:** the current workspace's uncommitted changes in Git, or the changes in jj's working commit. No registered workbench task is required. For existing PRs, the caller prepares this local workspace state before starting review. The product input is settled; precise VCS extraction and edge-case handling remain implementation details.

**Guide:** a narrated walkthrough through the diff, explaining the change in a useful reading order. A skill bundled with the tool drives the content structure and narration. Its English-language instructions can evolve through use. The document format is simple Markdown: headings, prose, and annotated fenced code/diff snippets. The renderer owns presentation and interactions; the skill owns the editorial approach.

**Backing agent:** Codex is the user's existing agent tool. The review command runs a Codex session with repository read access to prepare the review and act as a guide/advisor for Q&A. It can investigate surrounding code beyond the diff. Feedback export has no AI pass. It does not require a workbench supervisor. Session integration and continuation mechanics remain later technical-design questions.

**Session experience:** the user wants a full-featured Codex session through this web UI, retaining their configured MCP servers, instructions, skills, and normal tool capabilities. The browser must support the session's interaction needs, including streamed activity, approval/input requests, and interruption. Direct Codex app-server integration is the preferred direction, pending a live feasibility check. Review-specific guide, reference, feedback, and UI state remain owned by this application.

**Output:** a GitHub-review-like collection of code feedback, guide-prose feedback, and overall feedback, collated into one Markdown file. Code feedback references file paths and line ranges in the reviewed diff; guide-prose feedback quotes the selected passage alongside the user's comment; overall feedback has no required location. Export preserves recorded wording without AI summarisation, rewriting, or conversion into tasks. Concluding the review writes the file into the directory where the review was started, ready to give to an implementation agent or a human. Filename and overwrite behavior remain open.

## Review journey

1. **Understand the implementation:** present important changes in logical order and explain how they fit into the wider system. The browser guide is a notebook-like document interleaving explanatory prose and code diffs, with discussion alongside the document.
2. **Investigate:** highlight guide text or code diff and reference it in a freeform conversation with Codex. Selecting either prose or code automatically sets the active reference for the conversation; deselecting clears that reference. The conversation also supports references to any part of the diff, including changes absent from the guide and discovered in Zed. The backing agent can read the repository to explore beyond the excerpts.
3. **Inspect code and diff:** browse in Zed, preferably using its local, uncommitted-diff experience. Make moving between explanation and source easy.
4. **Record feedback:** select code or guide prose and directly write feedback. Code feedback attaches to the selected lines; prose feedback saves a quote of the selected passage alongside the comment. Also record overall feedback for the handover; its input placement remains open. Conversation alone does not add feedback.
5. **Conclude and hand over:** submit directly to complete the review and collate recorded feedback into one Markdown file in the starting directory. Feedback is visible in the UI throughout the review for the user to check; there is no separate preview, review, or confirmation step before conclusion. A CLI command such as `review conclude` may also expose this action; that remains open. If the approach is wrong, conclude with broad feedback early without finishing detailed review.

Prefer small changes to keep review and feedback manageable. The smallest useful journey is resumable: stopping and restarting must restore the entire review state. Stopping is distinct from explicitly concluding. Evolving-code workflows are open; the tool may only be needed for the first pass of short iterations.

## Discussion and handover

- Conversation has two reference types: a code file range (file path and line range), or guide text (the selected passage). Both use the same select-to-reference interaction while retaining their distinct context.
- One freeform conversation supports references to highlighted guide text and code diff; separate discussion threads and thread-resolution actions are outside the current interaction model.
- Conversation can reference any reviewed change, including code explored in Zed; reference capture mechanics remain deferred.
- Selecting code lines or guide prose automatically references the selection in the AI conversation until deselected or replaced by another selection. Deselecting clears the active reference for subsequent messages; previously sent messages retain their context, and recorded feedback retains its code location or quoted passage.
- Add feedback by directly writing a comment on selected code or prose. Prose feedback includes the selected passage as a quote alongside the comment, including in the Markdown handover. Selection alone does not record feedback.
- Only explicitly recorded feedback becomes handover content. Recording feedback collects it within the review; conclusion writes the combined file.
- Code feedback and overall feedback follow the familiar GitHub review model, extended with quoted guide-prose feedback. Markdown export simply collates recorded comments with their code locations or prose quotes, with no AI pass.
- Feedback on selections uses direct composition on code or guide prose. Recorded feedback remains visible in the UI for the user to check before submitting directly. Exact placement of overall feedback and editing controls remain UI-design details.

## UI direction

- A document resembling a Python notebook in reading flow: prose and code diffs interleaved in a logical review order. The notebook analogy describes presentation, not executable cells.
- The agent produces a Markdown guide interleaving prose with triple-backtick fenced blocks containing code or diffs. Each block is annotated with its source language and whether it is a diff. The agent supplies the block content; the tool renders HTML, layout, syntax/diff presentation, and interactive selection for conversation references or feedback.
- Each code/diff block has a header identifying its file path and covered line range. The tool uses this metadata to display source line numbers and provide copyable file/line references for opening the relevant code in Zed, as well as anchoring conversation references and feedback.
- Exact header/fence syntax remains open. Diff blocks need old/new source-line mapping, including separate ranges where needed, so deleted and added lines retain accurate locations. Define minimal metadata and validation during technical design. The earlier proposal to render code solely from agent-supplied references is superseded by agent-supplied fenced content.
- Highlight text or code to reference it in the freeform conversation. The earlier side-thread/comment model is superseded.
- A code or prose selection serves both actions: write feedback on the selection, or ask about it in chat with the reference already active. Deselecting clears the active chat reference.
- Freeform chat supports both discussion of highlights and broader questions, including references to changes outside the guide. Its placement is not yet settled.
- Feedback is written directly against selected code or prose. Recorded comments and overall feedback are visible during review. Conclusion is a direct submit action, without a separate review screen.

## Launch and persistence direction

- A CLI command starts or resumes a review and serves a browser UI through a local web server.
- Review state persists independently of the running command, server, and Codex process.
- Store review application state in `review.sqlite` in the directory where the review starts. This supersedes the home dot-folder and `.review/` proposals and makes the state easy to git-ignore and delete. Exclude this database and any SQLite auxiliary files from reviewed changes and change-triggered resets.
- Persist the entire review state: prepared guide, review actions, conversation and its references, recorded code/prose/overall feedback and quotes, unfinished chat and feedback drafts, reading position, active selection, and UI state. Restarting restores the review as the user left it.
- Provisional milestone 1 rule: changed code resets the review rather than carrying context forward. Precise reset behavior, including retention of the previous review, remains to be defined; evolving-code workflows are deferred.
- Whether a path can have multiple reviews remains open.

## Milestones

### 1. Resumable review through conclusion

This milestone is the prototype: a complete first working implementation of all agreed milestone 1 functionality that the user can install, test on real work, and refine. Integration experiments and fixture-based UI checks are development steps toward that deliverable, not substitutes for it.

Launch from a local repository, prepare an explanatory guide with Codex, discuss highlighted guide text or code diff in freeform chat, inspect code in Zed, explicitly record code, quoted-prose, and overall feedback, stop and resume, then conclude and write the Markdown feedback file into the starting directory. Only recorded feedback enters the handover, with no AI processing during export.

Completion should demonstrate that the entire review state survives a restart, recorded feedback is visible during review, and direct submission produces the Markdown handover without an AI pass or an extra review step. The provisional response to changed code is to reset the review. Acceptance criteria are below; precise reset semantics remain open.

### 2. Open — revisit after using milestone 1

No committed scope. Revisit after the user has used the built first milestone for some time. Carrying context across revisions, refreshing the guide, and reconciling feedback are possibilities to evaluate from experience, not requirements. For short iterations, the user may only use this tool for the first review pass.

## Milestone 1 acceptance criteria

1. **Understand:** launching from a repository reviews its uncommitted changes (or jj working commit) and opens a browser guide prepared by Codex using the tool's bundled walkthrough skill. The guide narrates the diff using Markdown headings, prose, and code/diff blocks. On a representative change, the user can explain the approach and how the important changes fit together, and locate the corresponding source for inspection in Zed.
2. **Investigate:** selecting code supplies its file and line range to the next chat message; selecting prose supplies the selected text. Replacing or clearing the selection updates the active reference without changing earlier messages. The user can also reference a diff range absent from the guide and ask Codex to investigate surrounding repository code.
3. **Record:** the user can directly add code feedback, quoted-prose feedback, and overall feedback, and see recorded feedback in the UI. Questions and answers do not enter the handover unless the user explicitly records feedback.
4. **Resume:** after closing the browser and stopping the command, relaunching on unchanged code restores the guide, conversation, references, feedback, unfinished drafts, reading position, selection, and UI state. An interruption during a Codex response leaves the request and any available response recoverable, with its interrupted status clear; exact continuation mechanics are deferred.
5. **Conclude:** one submit action completes the review and writes a Markdown file in the starting directory containing recorded comments with their code locations or prose quotes and overall feedback. Wording is preserved, with no AI pass or separate review step. The user can conclude early with only overall feedback.
6. **Handle changed code:** under the provisional reset rule, changed code starts a fresh review rather than presenting the previous guide and feedback as current. The treatment of previous state and the point at which changes are detected remain open reset details.
7. **Retain the Codex environment:** the review session can use the user's existing configured MCP servers and repository instructions, with tool activity and requests for approval/input surfaced in the web UI. Validate access with the same Codex configuration and authentication environment; do not assume every capability visible in a particular host app is automatically available to a separate local process.

## Implementation outline

The interaction-first sequence is accepted, subject to first validating that the necessary Codex integration is reasonably feasible. A small integration experiment precedes the UI work. The delivery target is the complete first working implementation; the experiments are internal validation steps.

Sequence within milestone 1, after the feasibility check:

1. **Prove the interaction loop:** use a representative review document to exercise prose/code selection, conversation references, direct feedback, visible recorded comments, and Markdown submission. Include persistence as each interaction is introduced.
2. **Connect the Codex experience:** bundle the narrated-walkthrough skill, generate the guide from actual workspace changes, and enable repository-aware freeform Q&A using both reference types, including ranges outside the guide. Exercise the workflow alongside Zed.
3. **Complete the lifecycle:** verify full restoration after shutdown, interrupted agent work, direct conclusion, and the agreed reset behavior. Refine the loop through a real review and check all acceptance criteria.

These are delivery slices within milestone 1, not additional product milestones. Rust/Tokio/Axum, Leptos CSR/Trunk, SQLite, and WebSocket application communication are agreed. Direct Codex app-server integration is preferred pending the feasibility check. Detailed crate boundaries, block metadata syntax, VCS extraction, and navigation mechanics remain technical-design work.

## Codex integration feasibility — updated 2026-10-18

The user considered ACP, then clarified the primary goal: talk to a full-featured Codex session through the review web UI, retaining registered MCP access. Direct app-server integration is now preferred. Zed's ACP support is irrelevant to this integration; Zed remains a code/diff viewer.

Initial documentation/source findings (subsequently confirmed by live tests; see implementation progress):

- Local installation: `codex-cli 0.163.0` exposes `app-server` and noninteractive `exec` with JSON events and session resumption.
- The [Codex ACP adapter](https://github.com/agentclientprotocol/codex-acp/blob/main/README.md) translates ACP requests into Codex app-server operations. It documents contextual prompts, streaming tool activity, authentication, and sandbox configuration.
- [ACP content](https://agentclientprotocol.com/protocol/v1/content) supports text and embedded resources. Our code-range and guide-text references can be supplied as explicit context; their selection behavior and persistence remain owned by this app.
- [ACP session loading](https://agentclientprotocol.com/protocol/v1/session-setup) is a negotiated capability. The adapter's [load implementation](https://github.com/agentclientprotocol/codex-acp/blob/main/src/CodexAcpClient.ts) resumes a Codex thread and reads durable history. This is evidence of support, not yet proof of restart behavior with a chosen released version.
- Direct [Codex app-server integration](https://learn.chatgpt.com/docs/app-server) is the alternative: it exposes persistent threads, streaming turns, interruption, read-only sandboxing, and per-turn structured output. ACP offers a common agent interface at the cost of an adapter dependency; direct integration exposes Codex capabilities without that translation layer.

MCP findings: [Codex MCP configuration](https://learn.chatgpt.com/docs/extend/mcp) is stored in Codex's user/project configuration. The app-server interface exposes MCP server/tool/auth status and configuration reload, along with approval and MCP input requests. The current [ACP adapter source](https://github.com/agentclientprotocol/codex-acp/blob/main/src/CodexAcpClient.ts) also reads existing MCP configuration and handles conflicts with client-supplied servers; ACP is not inherently incompatible with registered MCPs. Direct integration is preferred because the product specifically wants Codex's session capabilities without an adapter layer, not because MCP access is impossible through ACP.

Next feasibility experiment: use the installed Codex app server with the user's normal Codex configuration/authentication environment. Verify it can prepare a usable guide from a small local diff, answer follow-ups with both reference types, inspect surrounding code under read-only permissions, discover existing MCP tools and perform an appropriate read-only call, and reload the same conversation after the process stops. Also check approval/input request handling, cancellation/interrupted-response recovery, and whether the guide can be returned in a reliably usable format. Keep application review state separate from Codex's own configuration/session storage.

The review app must persist its own complete product state regardless of protocol; agent-session history alone cannot restore feedback, drafts, selections, or reading position.

## Other harnesses — feasibility comparison, 2026-10-18

Research only; adding other harnesses is not a milestone 1 commitment. No Claude or OpenCode integration has been run. The core review journey appears feasible with all three; differences concentrate in the agent connection, configuration, interactive controls, and recovery behavior.

| Harness | Native integration | Implications for this product |
| --- | --- | --- |
| Codex | [App server](https://learn.chatgpt.com/docs/app-server): persistent threads, streamed turns, tool events, approvals, MCP status/configuration. | Preferred first implementation; preserve the user's existing Codex environment. |
| Claude Code | [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview): runs the Claude Code harness from TypeScript or Python, with tools, MCP, hooks, and sessions. | Equivalent review workflow, with a different event/callback integration and explicit verification of configuration and authentication behavior. |
| OpenCode | [Headless HTTP server](https://docs.opencode.ai/docs/server/) and [SDK](https://docs.opencode.ai/docs/sdk/): sessions/messages, event streaming, permission responses, cancellation, and MCP management. | Similar architectural fit to Codex app server, using HTTP/events. The SDK retains OpenCode configuration while allowing overrides. |

Claude's current documentation says default SDK settings load user/project/local configuration and skills; [project MCP configuration](https://code.claude.com/docs/en/agent-sdk/mcp) loads when the project setting source is enabled. [Approval and question callbacks](https://code.claude.com/docs/en/agent-sdk/user-input) must be connected to our UI. [Session IDs](https://code.claude.com/docs/en/agent-sdk/sessions) allow resuming stored conversations. The SDK is not identical to every CLI feature: [agent teams](https://code.claude.com/docs/en/agent-sdk/claude-code-features), for example, are not directly configured through SDK options. Its overview directs third-party products to API-key authentication unless otherwise approved; reusing a Claude subscription must not be assumed.

OpenCode has its own [MCP configuration](https://docs.opencode.ai/docs/mcp-servers/). Its [native ACP interface](https://docs.opencode.ai/docs/acp/) explicitly supports configured MCPs, tools, project instructions, and permissions, though some built-in slash commands are unsupported. Claude also has an [ACP adapter](https://github.com/agentclientprotocol/claude-agent-acp). Thus ACP is a credible option if supporting several harnesses becomes a priority, but it does not eliminate differences in authentication, configuration, feature coverage, or restart behavior.

Architectural assessment: the guide, both reference types, feedback, Markdown export, and review UI persistence can stay shared. Each harness needs integration for starting/resuming a session, sending context, receiving messages/tool activity, answering approval/input requests, cancelling work, and handling failures. Configured MCPs and native conversation history belong to the chosen harness; using a different harness does not automatically carry over Codex registrations, credentials, or sessions. Using a provider's model through OpenCode also uses OpenCode's harness, not that provider's original harness.

Options to discuss: native integrations for maximum access to each harness's capabilities, or ACP for a common interface with adapter/capability constraints. Recommendation: keep the first implementation on Codex app server, separate the review's data and UI from Codex-specific session handling, and add a second harness only when needed. Supporting another harness should then be a focused integration plus behavior tests rather than a review-product rewrite. Full native feature parity remains materially more work than sharing the basic chat loop; no effort estimate is justified before the first integration experiment.

## Agent adapter direction — 2026-10-18

An agent adapter layer is the agreed architectural direction. It separates the review application from each harness's session and interaction API. The review should be able to use a different harness with relatively little integration work, while retaining the chosen harness's native configuration and capabilities: registered MCP servers, authentication, instructions, skills, tools, permissions, and session behavior. The adapter must allow harness-specific features to be exposed rather than restricting every harness to a lowest-common-denominator chat interface.

Switching harnesses should not require redesigning the guide, reference interactions, feedback, or export. Each harness continues to own its native configuration and agent-session state; this direction does not imply translating credentials/configuration between harnesses or migrating a live conversation from one harness to another. Codex remains the first target; additional harness implementations are not committed.

| Integration | Wire protocol / interface | Language implications |
| --- | --- | --- |
| [Codex app server](https://learn.chatgpt.com/docs/app-server) | Bidirectional JSON-RPC-style messages over subprocess stdin/stdout, newline-delimited. Also offers WebSocket transports; TCP WebSocket support is documented as experimental/unsupported. | Any language with process/JSON support. The installed CLI can generate TypeScript types or JSON Schema matching its protocol version. |
| [Claude Agent SDK](https://code.claude.com/docs/en/agent-sdk/overview) | Library API that runs the Claude Code binary, exposes streamed messages, and accepts callbacks for interactive controls. The [CLI](https://code.claude.com/docs/en/headless) separately supports JSON/streaming JSON for subprocess use. | Official harness SDKs are TypeScript and Python. Other languages can drive CLI subprocesses or bridge to an SDK process; full interactive behavior should not be inferred from simple CLI output streaming. |
| [OpenCode server](https://docs.opencode.ai/docs/server/) | HTTP/JSON endpoints, with Server-Sent Events for streamed updates; publishes an OpenAPI specification. | Any HTTP-capable language. The documented [SDK](https://docs.opencode.ai/docs/sdk/) is JavaScript/TypeScript. |
| [ACP](https://agentclientprotocol.com/protocol/v1/transports) | Common JSON-RPC protocol, normally newline-delimited messages over subprocess stdin/stdout. Streamable HTTP is currently a draft. | Language-independent wire protocol. Native support or a harness adapter is needed; this is an alternative integration surface, not our browser transport. |

Proposed responsibility split: the browser talks to our local review server; that server uses an agent adapter. The adapter handles session start/resume, messages with references, streamed text/tool activity, approvals/questions, cancellation, and errors. Keep harness-specific capabilities available rather than forcing every feature into a minimal common chat interface. Product state and feedback export stay outside the adapter.

Rust is chosen for the server/CLI and Leptos for the SPA to keep the stack simple. Ease of dependency installation, development, and CLI packaging takes priority. Codex's subprocess protocol and OpenCode's HTTP API are accessible directly from Rust; a future Claude adapter may use an SDK bridge or ACP adapter, with native capability coverage validated separately.

## Application stack — agreed

A Rust CLI/local server using Tokio and Axum, with a Leptos client-rendered SPA compiled to WebAssembly using Trunk. Share review/reference/event types in a small Rust crate. The server owns persistence, filesystem access, and harness adapters; the SPA owns document rendering and interaction. The packaged command serves the built UI assets so using the tool requires one application launch.

[CSR means client-side rendering](https://book.leptos.dev/getting_started/index.html): the browser loads the compiled Leptos application and builds/updates the document and controls locally. Axum serves the application assets and handles data and agent operations. Review state remains durable in the server-side SQLite file. This fits the local interactive tool without adding server-rendered UI and hydration.

Main UI feasibility check: selection across rendered prose and syntax-highlighted code, source-line mapping, and restoring that state. Leptos exposes [browser APIs through web-sys/wasm-bindgen](https://book.leptos.dev/web_sys.html), but these interactions still need deliberate implementation. Validate a small representative document before expanding the UI. This complements the Codex integration check. SQLite and WebSocket application communication are agreed; exact workspace layout and dependency versions remain open.

Browser API assessment: no essential `web-sys` capability gap has been identified for this review UI. Bindings exist for [Selection](https://docs.rs/web-sys/latest/web_sys/struct.Selection.html), [Range](https://docs.rs/web-sys/latest/web_sys/struct.Range.html), [Clipboard](https://docs.rs/web-sys/latest/web_sys/struct.Clipboard.html), and [EventSource](https://docs.rs/web-sys/latest/web_sys/struct.EventSource.html). Rust may require more DOM casts, feature flags, and callback lifecycle handling; this is an ergonomics tradeoff, not an established blocker or performance problem. Mapping selections to stable prose/code references and restoring them after rendering is application work in either framework. UI-library availability is not a decision driver for the planned scope. Leptos is preferred for stack simplicity; confirm selection behavior and the development/install workflow in the planned small prototype. Chat rendering and input are conventional components, with streaming, approval/input requests, and interruption recovery requiring careful state handling rather than a large UI framework ecosystem.

## Client/server communication — agreed

Use a bidirectional WebSocket stream between the Leptos SPA and local Rust server. This is separate from the server-to-agent adapter protocol.

[Axum supports WebSockets](https://docs.rs/axum/latest/axum/extract/ws/index.html). The application handles reconnection, state recovery, and commands whose outcome is uncertain after a disconnect.

Serve the SPA assets over HTTP and use one WebSocket per open UI for interactive application traffic. Shared Rust message types represent user commands (send prompt, record feedback, save draft/position, answer an approval, interrupt, conclude), responses/errors, and server events (text/tool updates and review changes). Use JSON encoding initially.

Initial load: show the application shell with a loading indicator, connect the WebSocket, receive a current-state snapshot, then render and restore the guide, chat history, feedback, unfinished drafts, reading position, and selection. Follow the snapshot with incremental live updates. A new review initially shows that the guide is being prepared. Refresh and reconnect use the same restoration flow.

Reliability direction: the server owns authoritative review state; acknowledge saved changes after persistence; give commands IDs so reconnect retries can be deduplicated; restore a consistent current-state snapshot on connection/reconnection before following live updates. A browser disconnect leaves the server's review and agent session intact while the CLI remains running. Details of transient streaming buffers, batching frequent UI changes, pending command reconciliation, and interruption during server shutdown remain technical-design work. Local operation reduces deployment complexity but does not remove refresh, sleep, or restart scenarios.

## Development, installation, and local storage

Agreed priorities: dependencies should be easy to install, a development environment should be easy to run for testing, and the finished tool should install as a CLI command that can simply be launched. All review application state must persist in local files.

Agreed developer experience: a documented bootstrap for the Rust toolchain, WASM target, and Trunk, followed by one development command that starts the server and UI rebuild/watch workflow. Provide one local installation command that builds the UI and installs the CLI. Exact commands are to be established, not assumed to exist. [Trunk](https://trunk-rs.github.io/trunk/guide/getting-started/installation.html) supports Cargo, prebuilt binaries, and package-manager installation.

Maintain a root `justfile` as the user-facing entry point for useful automations: dependency setup/checks, development, running the review tool, formatting/linting/tests (including browser tests), building, and local CLI installation. Give recipes descriptions so `just --list` explains the available commands. Document the initial installation of Just itself. Keep recipes working as implementation evolves; the user should not need to reconstruct build/install/run steps. The installed agent harness remains an assumed external dependency.

Agreed distribution direction: embed the built SPA assets in the CLI binary and bundle SQLite. The agent harness is assumed to be already installed and configured; installing or bundling it is outside this tool's scope. A prebuilt release uses that harness and a browser at runtime. A source/local install must build or include the UI assets before installing the executable; `cargo install` alone does not define that asset build. [Rusqlite's bundled option](https://github.com/rusqlite/rusqlite) compiles and links SQLite with the application.

Storage direction: a local `review.sqlite` file in the review's starting directory holds all application-owned persistent state. Account for any SQLite auxiliary files in ignore and cleanup behavior. Database schema, journal mode, and exact cleanup mechanics remain implementation details.

The harness continues to own its native configuration and session files. The application stores its own conversation display/history and a native session reference; deleting review state does not automatically delete the harness's history. A completely self-contained portable review would require additional session-storage work and is not established by choosing SQLite.

## Boundaries and preferences

- Own guided understanding, conversation, and feedback preparation. The caller owns workspace setup, PR handling, publication, and acting on feedback.
- Keep input/output simple while supporting an interactive experience. Conversation and feedback-export mechanics should evolve through use.
- Zed is the companion for full code and diff exploration; the browser provides explanation, selectable code/diff, freeform conversation with highlight references, and explicit feedback recording.
- Prefer local operation and editable agent instructions/skills. Respect project-local instructions through the harness where supported.
- Start evolving workflows as English-language skills and turn established behavior into software. Specialized review UI can be built directly.
- Linear-style guided review is an experience reference, not a feature commitment.

## Open decisions and next steps

Implementation readiness: the prototype means the complete, usable milestone 1 implementation, covering all agreed functionality and acceptance criteria. The input is current workspace uncommitted changes / jj's working commit; the guide is a narrated Markdown walkthrough driven by a bundled skill. No further broad product or framework decision is needed before implementation planning.

Prepare the executable plan around the full journey, including the bundled skill, minimal block-header/fence syntax, native Codex integration, interactive UI, SQLite/WebSocket restoration, feedback export, and setup/dev/install commands. Integration experiments and fixture documents can validate risks early; delivery must exercise the complete workflow on real changes. Milestone 2 remains open.

Crate names, database schema, message names, exact block metadata, asset packaging, and VCS extraction can be chosen during implementation. Reset/archive semantics and completed-review behavior need settling before milestone 1 is complete; they should not silently reduce the prototype's scope.

Implementation discretion: the user delegates routine implementation choices and accepts revising them after testing. Proceed with reasonable, documented defaults; return for input only if a finding materially changes the agreed experience, scope, or feasibility. For lifecycle details, prefer retaining previous work and avoiding silent overwrites while keeping the agreed fresh-review behavior on changed code.

Testing direction: write automated tests alongside development, including Rust tests for state, references, VCS extraction, and export; browser automation for actual selection, chat, feedback, and restoration; and a live Codex integration check using the existing configuration. Exercise browser refresh/disconnect, server restart, interrupted agent work, changed code, and the installed CLI. Test dependencies are development-only. Controlled harness fixtures can cover failure and approval/input paths reliably, but do not replace the live Codex/MCP check. Use headless Playwright MCP for exploratory and acceptance testing of the running application against milestone 1 criteria, alongside the automated development tests. Repeatable test-suite setup remains implementation work. Repository working-method guidance is recorded in [AGENTS.md](AGENTS.md).

Testing tooling verified 2026-10-18: Microsoft's [Playwright MCP](https://github.com/microsoft/playwright-mcp) is connected, including coordinate-based mouse tools. Successfully opened `about:blank` and evaluated page readiness and browser Selection API availability. This verifies browser access, not the application's interactions or localhost connectivity, which remain to be tested during implementation. It complements repeatable automated tests and is not a runtime dependency of the review tool.

Prefer headless browser testing to avoid opening windows during development. Playwright MCP was reconfigured with `--headless` and verified on 2026-10-18: `about:blank` loaded successfully and the browser reported `HeadlessChrome` in its user agent.

The integration feasibility check and minimal reset semantics are now settled; see implementation progress below. Further iteration should be driven by using milestone 1.

During implementation outline and technical design: bundled walkthrough skill instructions; Markdown fence annotations for language, diff status, source-file/line mapping, and validation of agent-supplied code/diffs (HTML rendering and interactions owned by the tool); overall-feedback placement and editing controls; extraction of Git uncommitted changes / jj working-commit changes; stable file/line references; guide-to-Zed navigation and reference capture; harness/session integration; full-state persistence; reset mechanics; conclusion command and output filename. Human interaction is required; whether the review tool needs its own MCP interface remains open. Revision support is deferred pending experience with milestone 1.

Planning path: product outline → milestones → implementation outline → technical design and executable plan. Product and stack decisions are agreed; routine implementation decisions are delegated as described above. Proposed success signals: faster understanding and judgement, easy return after interruption, and actionable feedback requiring little manual repackaging.

## Implementation progress — 2026-10-18

Implementation is underway in three Cargo crates: `review-core` (shared state, references, verified Markdown rendering), `review-server` (the `review` CLI, Axum server, SQLite store, native harness adapter), and `review-ui` (Leptos CSR). The walkthrough skill lives in `skills/review-walkthrough/SKILL.md`. Root `justfile` recipes cover setup, development, build, installation, checks, and automated/live/browser tests.

Settled lifecycle defaults: one active review per starting directory; changed code archives previous state in the same SQLite database and opens a fresh review; changes are detected only when the command starts, with automatic guide preparation. Completed reviews reopen as completed. There is no in-page restart; `review history` lists retained reviews. `review conclude` is supported while the server is stopped. Submission writes `review-feedback-<review-id>.md` without overwriting different existing content. Application state, lock/temporary files, and exported handovers are excluded from input/change detection. Git input combines the current working tree and index against HEAD, including untracked files; jj uses `@`. Launching in a subdirectory still reviews the complete repository.

Snippet syntax: a fenced language followed by JSON metadata, e.g. `rust {"file":"src/main.rs","start":12}` or `rust {"file":"src/main.rs","diff":true}` with unified-diff hunk headers in the body. The tool verifies source text and old/new line mappings against the captured workspace; invalid mappings remain visible with a warning and have no interactive source anchors. Any reviewed file range can also be referenced explicitly from the conversation controls.

Native Codex feasibility is verified against installed version 0.163.0: guide generation with valid source annotations, code/prose follow-ups, discovery of existing configured MCPs, an approved read-only Playwright tool call, and conversation resumption after app-server restart. Some pre-existing MCP registrations report authentication errors (Sentry and rayon-admin-api); the successful Playwright check establishes native configuration reuse without assuming all registered servers are currently authenticated. The actual server/browser flow has also generated and discussed a guide using Codex on a sample Git change.

Verification: 11 automated Rust tests pass (including Git/jj extraction, persisted state, export, source validation, and native event handling); the separate opt-in live Codex test passes; five headless browser acceptance tests pass. Browser coverage includes code/prose references, manual ranges, chat, feedback editing, drafts, direct export, server kill/restart, changed-code archival, approval/question/form controls, interrupted partial responses, duplicate-command protection, and reading-position restoration. Formatting and Clippy checks pass for both server and WASM UI. `just install` builds and installs the CLI, and the installed binary successfully served its embedded UI from outside the checkout. A real Codex-backed browser review generated a narrated guide, answered a selected-code follow-up, recorded feedback, and exported Markdown. Milestone 1 is ready for user testing and refinement; milestone 2 remains open.

Operational details: the server remembers its port in SQLite so browser tabs can reconnect after restart; `--port 0` explicitly chooses another. Local WebSockets require a matching loopback origin. A separate advisory lock prevents concurrent servers/exporters for one review. Agent protocol translation lives in `agent.rs`, exposing normalized review events while retaining native request payloads. Startup failures and interrupted work remain visible and recoverable; uncertain browser commands retain stable IDs in a local outbox.

The development workflow was also smoke-tested: `just dev <repository>` starts the server and Trunk watcher, prints the local URL, and shuts down with Ctrl-C.


## First review feedback — 2026-10-19

The first real review cycle completed successfully. Feedback from `review-feedback-ff635916-1c04-45ff-8a66-aafc0e0f5727.md` settles these refinements:

- Crate package names match their directories: `review-core`, `review-server`, and `review-ui`; the executable remains `review`.
- Capture changes only when starting the review command. Keep the running page on its captured snapshot, with no polling or per-action recapture. Changed code at the next launch archives the previous review and automatically prepares a new one. CLI conclusion exports the saved snapshot without starting a fresh review.
- Each page is a single review. Remove New review and Prepare/regenerate controls; preparation starts automatically, with a retry only when no guide is available after interruption or failure. Submission saves the Markdown handover, shows a prominent completion message, prints the path, and exits the command.
- Put conversation/feedback on the left and the guide on the right. Use a technical, sans-serif presentation with a Catppuccin-inspired light palette. Keep code and prose references visibly highlighted while typing and after restoration; place the active reference beside the relevant input.
- Guide preparation belongs in a loading/skeleton state in the document, not the chat history. Native requests remain available during preparation. The conversation retains the native session context for follow-up questions.
- Overall feedback is implicit when no reference is selected. Multiple overall comments remain supported.
- The bundled skill must walk through the mechanics and majority of significant changes, especially addition-heavy diffs. Prefer code identifiers and ordinary language; explain necessary new terms using rendered Markdown definition lists.
- `AGENTS.md` now prefers cohesive types/impl blocks and blank lines between logical steps. Milestone 2 remains open.

Verification for these refinements: 14 Rust tests pass; formatting and Clippy pass for server and WASM. Eight browser regression tests cover the revised layout, persistent highlights across input focus/reload, multiple implicit general comments, startup-only reset, export followed by process exit, CLI export after code edits, and a conflicting export file without data loss. Headless Playwright MCP also exercised code/prose selection, chat, feedback recording, and completion. The skill validator passes. A live Codex run on two newly added Rust files produced 19 source excerpts with no mapping warnings, covering data structures, transactional persistence, reference validation, export, and tests. Preparation showed the loading state with zero chat messages. A selected-code follow-up used the same native session; feedback submission then exported and exited successfully. The installed `review` command has been updated.


## Preparation approval visibility — 2026-10-19

Reproduced command and MCP approval buttons falling outside the visible panel during initial preparation at a 600px viewport height. Pending requests now occupy a separate, sticky, scrollable region at the top of the side panel, independent of conversation/feedback tabs and the activity log. The preparation state explicitly reports waiting for a response. Ten browser tests pass, including native-protocol test peers that pause initial generation until approval, decline/retry, and request restoration after a browser reload. These peers exercise the real stdio transport and response path without calling a model or automatically approving a user's tools. Formatting and Clippy pass. Headless Playwright MCP also verified visible approval at 1280×600, reload, approval delivery, guide completion, and submission. The installed CLI includes the fix. The user's current saved review had no pending native request when inspected; the visibility failure was reproduced independently.

Follow-up diagnosis: the reported message was seen in Codex activity. The saved native events contained MCP reauthentication failures for `sentry` and `rayon-admin-api`, no approval requests, and successful guide completion. Reauthentication is distinct from approving a tool call; the harness reports `codex mcp login <server>` as its recovery action. Other startup failures included missing GitHub environment credentials and a Rayon OAuth metadata mismatch. No credentials or native configuration were changed.


## Second review feedback — 2026-10-19

Refinements from `review-feedback-e9225da2-7d67-4bb2-a20f-898cc25e8cef.md`:

- Use the guide's AI-generated title as the document title; remove the generic “Understand the change” block while retaining repository path and VCS statistics. The bundled skill asks for a concrete summary title.
- Open the browser once the guide is ready. Open earlier only for native requests or preparation failures requiring interaction; print the URL immediately. Resuming a prepared review opens immediately.
- Persist paragraph selections using DOM ranges rather than assuming their anchor is a text node. Support explicit code-row dragging and Shift-click ranges, including reverse selection and restored anchors. Keep highlighting reactive to selection changes.
- Show the captured prose or code in sent chat messages as well as passing it to the harness. Enter sends chat/records feedback; Shift+Enter adds a newline. Ignore Enter during IME composition and prevent held Enter from repeating submission.
- Replace domain strings with `ReviewStatus`, `MessageRole`, `MessageStatus`, and `DiffSide`. Standard JSON names stay unchanged; accept legacy message `completed` and empty manual diff-side values on load. Unknown native turn statuses become explicit failures at the adapter boundary.
- Keep lib/main entry points small. Split shared models/reference/protocol/export, server launch/transport/commands/session, and UI document/chat/feedback/selection/requests into focused modules. Use named handlers for substantive UI actions; record the convention in AGENTS.md.
- VCS capture already respects `.gitignore` for untracked files. Add coverage for nested ignore rules and negation in Git and jj. Retain tracked changes even if a later ignore rule matches them, as the VCS does.
- Replace the Python development watcher with Rust `xtask`, exposed through the same `just dev` command. The optional browser-test protocol peer uses Node, already required by Playwright, so Python is no longer a project dependency.

Verification: 20 Rust tests and 13 headless browser tests pass; formatting, Clippy (including the WASM UI and xtask), and skill validation pass. New regression coverage includes whole-paragraph selection across inline markup, persistent highlights and code anchors, drag/reverse Shift-click ranges, releasing a drag outside the document, old/new diff context lines, quoted chat, Enter/Shift+Enter and IME composition, ignore rules, serialized enum compatibility, unknown harness statuses, and browser-opening readiness. Headless Playwright MCP exercised approval, prose/code selection, chat, multiline feedback, restoration after a Rust watcher rebuild, and submission; the Rust development runner exited cleanly after export. These checks used controlled harness fixtures; the previously verified live Codex/MCP integration was not rerun for this refinement.

`just install` rebuilt the release UI and updated the installed `review` CLI with these changes.

## Preparation progress — 2026-10-19

Show elapsed preparation time in both the terminal and browser. The server persists the start time for each attempt; browser refresh retains the clock, retry resets it, and completion/interruption hides the running browser clock. Elapsed time includes waiting for approval/input, with an explicit waiting status. This is a clock, not a completion estimate. Keep the existing delayed browser-opening policy and print the URL for early access. Interactive terminals update one line every second; redirected output reports state changes and a heartbeat every 15 seconds. No Codex launch or permission settings change.

Verification: 21 Rust tests and 14 browser tests pass, including clock persistence, older-state compatibility, reload continuity, retry reset, stopping on completion, and terminal waiting/completion messages. Formatting and Clippy pass. Headless Playwright MCP verified the running browser clock, refresh continuity, approval and completion; a PTY verified the terminal's in-place elapsed updates. Tests used the native-protocol fixture; Codex integration settings are unchanged.
