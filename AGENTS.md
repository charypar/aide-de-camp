# Working in Aide

- Read the relevant product plan before working: `plans/REVIEW_PLAN.md` for guided review, `plans/COORDINATION_PLAN.md` for the companion workbench. Keep plans current as decisions settle. The tools belong to one Cargo workspace and remain independently usable.
- Make routine implementation choices autonomously and document useful defaults. Ask for input when a finding materially changes the agreed experience, scope, or feasibility. Keep discussion succinct and high-level; discuss options before changing product direction.
- Write meaningful automated tests alongside implementation. Cover observable behavior, persistence, protocol boundaries, and failure/recovery paths; add regression coverage for bugs. Run the relevant tests and checks before considering work complete.
- Use headless Playwright MCP for browser acceptance testing against the running application. Exercise real selection, chat, feedback, and restoration flows against the plan's acceptance criteria. This complements automated development tests. Use controlled harness fixtures for repeatable edge cases and live Codex integration checks for native session/configuration/MCP behavior. Report what was actually verified and any remaining gaps.
- Maintain a root `justfile` with documented recipes for dependency setup/checks, development, running, formatting/linting, tests, building, and CLI installation. Keep `just --list` useful and document how to install Just itself. The agent harness is an already-installed external dependency.
- Deliver the complete agreed milestone 1 as the prototype. Feasibility experiments and fixture-based demos are development steps, not the deliverable. Leave milestone 2 open until experience with milestone 1 informs it.

- Prefer cohesive types and `impl` blocks for behavior with shared state or responsibilities over collections of free functions. Keep small pure helpers where they improve clarity. Use blank lines between logical steps in longer blocks and between blocks; avoid compressed multi-step expressions and dense event handlers.

- Keep crate entry points small; put models, lifecycle/session handling, transport, and UI concerns in focused modules. Prefer named event handlers over substantive inline closures in UI markup. Use enums for finite domain states and convert native protocol strings at the adapter boundary.
