# AI configuration in the workbench

Status: implemented, 2026-11-13. A small read-only browser for the private configuration described in [the coordination plan](COORDINATION_PLAN.md#personal-and-shared-configuration). Editing and creating files happen in Zed.

## Experience

Replace the static “Tasks” header subtitle with section links: **Tasks** and a cog icon for **AI configuration**. The icon has an accessible name and tooltip. Indicate the active section visually and with `aria-current`. Keep connection status, task polling and browser-title attention counts working in both sections. Returning to Tasks preserves project filtering, completed visibility and expanded rows.

Use hash navigation (`#/tasks`, `#/configuration`, and an encoded selected-file identifier) for reload and Back/Forward support without changing static-asset routing. The existing root URL opens Tasks.

AI configuration shows the configured `$AIDE_HOME/agent-config` location, an **Open folder in Zed** action, a filename/path filter and a compact file list beside a read-only text viewer. Stack the list and viewer on narrow screens. Show relative paths so similarly named files remain distinguishable. Include `methodology.md`, `projects.json`, and text documents under `projects/`, `skills/` and `roles/`, including skill supporting documents. Omit VCS internals and binary assets.

Selecting a file displays its plain text with line numbers. Long lines soft-wrap, including unbroken paths, while preserving whitespace and original line numbers for copied references and Zed. Provide **Open in Zed** for the selected file and **Copy reference** using the review tool’s `path:line` convention. Use an absolute path because these files live outside task workspaces; default to line 1 and allow clicking a line number to choose another line. The Zed action opens that file at the selected line. Show a brief copy acknowledgement or clipboard error.

The filename/path filter finds files; browser Find handles text within the displayed document. No indexed full-text search is needed. An explicit **Refresh** reloads the list and selected document after external edits. Refresh on section entry and reconnect as well. Preserve selection where possible and show a clear missing-file message if it was removed. Ignore late responses for previously selected files.

A short note explains that personal rules apply when a task first captures its instructions; existing snapshots stay unchanged. Skills and roles in this directory are not automatically loaded by Aide. Shared repository instructions and externally installed skills remain outside this view.

If the configuration directory is absent, show its expected path and explain that it can be created externally. Browsing must not create directories or initialize a repository. If Zed is unavailable or a launch fails, show the error and retain the copy-reference/path fallback.

## Implementation

1. Add a focused configuration service in `aide-server` for bounded listing/reading and opening the configuration folder or selected file in Zed. Reuse the existing external-editor launch convention, passing paths as process arguments. Keep file identifiers confined to the configuration root; reject traversal and symlink escapes. Apply the existing local-origin/token authorization. No configuration writes or instruction-loader changes are needed.
2. Add small typed list/read/open messages in `aide-core` and request-correlated responses over the existing WebSocket connection. Fetch document content on demand, outside recurring task snapshots. Bound traversal and document sizes; unreadable, oversized or non-UTF-8 files produce a local error without breaking the list. Treat `projects.json` as viewable text, so invalid mappings do not prevent browsing.
3. Split the application shell and Tasks view as needed, then add navigation and a focused configuration browser component. Keep filter, selected file and selected line as simple view state; no editor dependency, draft persistence or filesystem watcher is needed.
4. Update README usage and the coordination plan after implementation. Add a focused test recipe to the root justfile if the existing recipes do not cover the new checks.

## Verification

- Add meaningful service/protocol tests alongside implementation for listing, reading, missing/unreadable files, bounded reads, path confinement, authorization and exact Zed arguments, including paths with spaces. Verify browsing and opening do not modify configuration files.
- Browser tests cover section links, Back/Forward/reload, file filtering and selection, line references and clipboard feedback, refresh after external edits/deletion, reconnect, errors and mobile bounds. Task filters, expanded rows and attention counts must continue working.
- Use headless Playwright MCP against the running application to exercise find → view → copy reference and folder/file launch requests with a controlled launcher fixture. Distinguish fixture launch verification from any manual/live Zed check in the report.
- Run relevant formatting, lint, build and dashboard regression checks. Native instruction loading and guided review behavior are unchanged; report those integrations as not rerun unless implementation touches them.

## Scope boundary

No in-browser authoring, save/create/delete operations, mapping forms, conflict handling, Git/jj controls, skill installation or changes to task instruction snapshots. Users adjust and create files in their external editor, then refresh this view. Milestone 2 remains open.

## Delivery verification — 2026-11-13

Implemented header navigation, a filename/path filter, read-only text with selectable
line numbers, absolute reference copying, Zed folder/file actions and refresh on
entry, reconnect and request. Configuration browsing does not change instruction
loading or write files. Text reads are limited to 256 KiB and 10,000 newline
characters; traversal is limited to 2,048 entries and 12 directory levels.
Symlinked files/directories are excluded; content reads use directory-relative
no-follow opens. Supported text extensions are md, txt, json, toml, yaml, yml, rst,
sh, py, js and ts.

The full Aide Rust suite passed (47 tests, two optional live tests skipped), followed
by three passing focused configuration tests after read-confinement hardening and
additional unreadable-file/limit coverage. All 36 dashboard, configuration, GitHub
and instruction browser tests passed. Formatting, Aide server/UI Clippy, dashboard
build and server build passed. Headless Playwright MCP independently verified
filtering, viewing, copying line 2, folder/file launch requests, refresh/reload and
390px mobile bounds. Editor launches used a controlled fixture; actual Zed GUI
behavior and native Codex/GitHub integrations were not rerun. Guided review
selection/chat/feedback flows were unchanged and not rerun.

Header and wrapping refinement — 2026-11-13: configuration now uses a cog link
with an accessible name, tooltip and active indicator. Long source lines wrap,
including unbroken paths, with whitespace and original line references preserved.
All six configuration browser tests passed, including keyboard navigation and
1440px/390px wrapping checks. Headless Playwright MCP independently confirmed
no horizontal overflow at both widths and correct copying of line 2. UI/server
builds, formatting and UI Clippy passed. Backend and native integrations were
unchanged and were not rerun for this presentation change.
