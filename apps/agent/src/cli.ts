/**
 * Terminal chat with the designer agent (`pnpm chat`).
 *
 *   pnpm chat                 interactive; empty line or Ctrl+D quits
 *   pnpm chat "question..."   one-shot: print one answer and exit
 *
 * Each run is one conversation thread. History lives in memory only, so it is
 * lost when the process exits. The world server must be running for scene
 * questions (`cargo run -p tflop-server`); otherwise the tools report
 * engine_unreachable and the agent says so.
 */
import { randomUUID } from "node:crypto";
import { createInterface } from "node:readline/promises";
import { stdin, stdout } from "node:process";

import { ask, buildAgent, DEFAULT_MODEL } from "./agent.js";

if (!process.env.ANTHROPIC_API_KEY) {
  console.error("ANTHROPIC_API_KEY is not set. Add it to the repo-root .env (see .env.example).");
  process.exit(1);
}

const agent = buildAgent();
const threadId = randomUUID();
const oneShot = process.argv.slice(2).join(" ").trim();

if (oneShot) {
  console.log(await ask(agent, threadId, oneShot));
} else {
  console.log(`tflop designer agent (${process.env.DEEP_AGENTS_MODEL || DEFAULT_MODEL}). Empty line or Ctrl+D to quit.`);
  const rl = createInterface({ input: stdin, output: stdout });
  try {
    for (;;) {
      const line = (await rl.question("\nyou> ")).trim();
      if (!line) break;
      console.log(`\nagent> ${await ask(agent, threadId, line)}`);
    }
  } catch (error) {
    // Ctrl+D closes stdin and rejects the pending question.
    if ((error as { code?: string }).code !== "ABORT_ERR") throw error;
  } finally {
    rl.close();
  }
}
