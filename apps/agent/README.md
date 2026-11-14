# Designer agent

LangChain Deep Agent (TypeScript) for tflop. It reads and edits the room through
the Rust world server (`crates/server`) using four tools: `get_scene`,
`list_assets`, `apply_edits`, `validate_layout`. See `../../docs/AGENT_TOOLS.md`
for the engine contract and `../../DESIGNER.md` for the operating contract.

Current engine status: `get_scene` and `list_assets` work; edits and
validation return `not_implemented` until world-core implements them. The
agent reports that plainly instead of retrying.

## Run

Requires Rust (`cargo`) and Node 22 + pnpm. Add `ANTHROPIC_API_KEY` (and
optionally `DEEP_AGENTS_MODEL`, default `claude-opus-5-5`; `RUST_ENGINE_URL`,
default `http://127.0.0.1:3000`) to the repo-root `.env`.

```bash
# terminal 1, repo root: serves the built-in demo room (or --scene <file.json>)
cargo run -p tflop-server

# terminal 2
cd apps/agent
pnpm install
pnpm chat                                   # interactive; one thread per run
pnpm chat "What furniture is in the room?"  # one-shot
```

The server keeps the scene in memory; restarting it resets all edits.

## Check

```bash
cargo build -p tflop-server   # needed for the contract tests
pnpm test                     # no API key needed (scripted fake model)
pnpm typecheck
```

`test/agent.test.ts` checks the conversation thread, the system prompt, and the
exact set of tools bound to the model (no `task` subagent, no `execute` shell).
`test/engine.contract.test.ts` starts the real server binary and checks the
scene summary, that every edit type is accepted by the server's JSON types,
error handling, and a `get_scene` call inside the agent loop. It is skipped if
the binary has not been built.
