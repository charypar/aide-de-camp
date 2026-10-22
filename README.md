# DevDeck

A local guided review tool. Codex narrates your current Git uncommitted changes or jj working commit. Select guide text or source lines to discuss them, record explicit feedback, and submit one Markdown handover.

The Cargo workspace also reserves room for the independently usable coordination workbench described in `COORDINATION_PLAN.md`.

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
review
```

The installed CLI embeds the UI and SQLite; it needs no Trunk or Node runtime. It launches the already-installed Codex harness with your normal configuration and authentication. The browser opens when the guide is ready; approval/input requests and preparation failures open it earlier so you can respond. The URL is printed immediately if you want to open it yourself. Review sessions use a read-only sandbox with approval requests shown in the UI. You can interrupt work and answer native harness requests in the browser. Pending approvals and questions appear at the top of the side panel, including while the initial guide is being prepared.

Preparation shows an elapsed clock in the terminal and browser, with a distinct status when Codex is waiting for your response. Browser refresh preserves the clock; retry starts a new attempt. Elapsed time includes approval waits and is not a completion estimate. Redirected terminal output reports state changes and progress every 15 seconds.

`review --no-open --port 4317` prints the URL without opening a browser. `review --path /path/to/repository` starts elsewhere. Stop with Ctrl-C and run again to resume. The server remembers its port so existing browser tabs can reconnect; use `--port 0` to choose a different port if necessary. To conclude without the browser, first stop the server, then run `review conclude`. `review history` lists archived reviews until conclusion cleans up the local database. `review --fixture` provides a clearly deterministic harness for UI development without model calls.

## State and feedback

Application state lives in `.review/` in the starting directory: `review.sqlite`, the lock, and temporary files. The folder is ignored by the review input and contains its own Git ignore file. Add `.review/` and `review-feedback-*.md` to the repository's ignore file if desired.

The complete repository workspace is reviewed even when launched in a subdirectory. Git input combines staged, unstaged, and untracked changes against HEAD. In jj it uses the working commit `@`. Both respect `.gitignore` for untracked files, including nested rules and negation; already tracked changes remain part of the review, matching VCS behavior. Source mappings are verified against a captured snapshot; binary changes are represented in the diff and described in prose.

The workspace is captured once when `review` starts. Edits during a running session do not reset it; stop and relaunch to review changed code. On relaunch, changed code archives the previous review and automatically prepares a fresh guide. Archived reviews remain in SQLite until conclusion; this first milestone does not rebase feedback onto evolving code. Successful conclusion removes the whole `.review/` folder, including temporary assets, leaving the Markdown feedback file in the starting directory. A later `review` run starts fresh, even on unchanged code. Failed exports preserve review state. Each page represents one review, with no in-page restart or regeneration controls. If preparation fails or is interrupted, **Retry preparation** retries the missing guide. `review conclude` exports the saved review even if code has changed since it stopped.

The walkthrough sits beside a collapsible chat panel; the panel icon beside the DevDeck name toggles it. Select guide prose or code to reveal two actions: **Add feedback** opens a wide, flat comment editor in the right margin, flush with the page edge; **Discuss with agent** opens chat, attaches that reference, and focuses the composer. Selecting alone does not change chat context. Recorded feedback stays highlighted until deleted; clicking a highlight focuses its comment. Comments remain editable, and overlapping comments stack without covering each other. Inline editors have a white background; opening one preserves the document’s reading position.

Drag or triple-click to select prose; drag across code rows or click a row then Shift-click another to select a range. Sent chat messages show the quoted passage or captured code excerpt. Enter sends chat or records an inline comment; Shift+Enter inserts a newline. Use **Reference a file range** in chat for changes absent from the guide, including ranges found in Zed. Source references include an old/new side for deleted lines. **Copy reference** copies `path:line` for opening in your editor. Chat follows streamed text while you are at the bottom; scrolling up pauses following until you return to the bottom or use **Jump to latest**. Pending requests remain signalled in the header when chat is collapsed.

Inline comments have a **Change / Question** toggle. The collapsible bottom drawer contains one uncategorised **Overall feedback** field, saved as you type, and **Submit review**. Inline drafts survive interruptions but only recorded comments enter the handover. Overall feedback is included directly. Panel state, drafts, and highlights survive restart.

**Submit review** writes `review-feedback-<review-id>.md` without an AI pass or extra confirmation. If there is no recorded feedback and the overall field is blank, submission or `review conclude` instead reports “Review concluded. No feedback recorded.” and removes the review state without creating a file. Exports contain **Overall feedback**, then **Changes** to implement, then **Questions** to answer and discuss or plan. Each inline comment retains its source reference or quoted guide passage. Wording and comment order within each group are preserved; empty groups are omitted. A completion message shows the export path, and the CLI exits. Existing files with different contents are never overwritten.

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

Automated tests cover source mappings, persistence, exports, VCS input, and browser workflows. The live check uses your configured Codex account and MCPs, and may incur normal model usage. Headless Playwright MCP is also used for interactive acceptance testing. See `REVIEW_PLAN.md` for scope and verification status.
