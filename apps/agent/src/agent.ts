/**
 * Construction of the tflop designer agent: a LangChain Deep Agent running on
 * Claude whose only side effects go through the Rust world server's tools.
 *
 * Bound tools (asserted in test/agent.test.ts):
 * - engine tools from ./tools.ts: get_scene, list_assets, apply_edits,
 *   validate_layout;
 * - write_todos (planning), added via todoListMiddleware;
 * - deepagents' virtual-file tools (ls, read_file, write_file, edit_file,
 *   delete, glob, grep), which only touch files held in conversation state,
 *   never the host disk.
 * The subagent `task` tool and the shell `execute` tool are excluded.
 */
import { ChatAnthropic } from "@langchain/anthropic";
import type { BaseChatModel } from "@langchain/core/language_models/chat_models";
import { MemorySaver } from "@langchain/langgraph";
import { createDeepAgent, registerHarnessProfile } from "deepagents";
import { todoListMiddleware } from "langchain";

import { EngineClient } from "./engine.js";
import { DESIGNER_PROMPT } from "./prompt.js";
import { createEngineTools } from "./tools.js";

/** Claude model used when DEEP_AGENTS_MODEL is unset. */
export const DEFAULT_MODEL = "claude-opus-5-5";

// DESIGNER.md forbids general-purpose subagents and a shell. When a model
// instance is passed, deepagents resolves the harness profile from the class
// name (ChatAnthropic -> "anthropic"), so register under the provider key.
// deepagents 1.16.5 always installs the subagent middleware, so the `task`
// tool is bound even with zero subagents; hide it explicitly. `execute` only
// exists with a sandbox/shell backend (we use neither), excluded defensively.
registerHarnessProfile("anthropic", {
  generalPurposeSubagent: { enabled: false },
  excludedTools: ["task", "execute"],
});

/**
 * Create the Claude chat model. Reads ANTHROPIC_API_KEY from the environment.
 *
 * Opus 5.5 rejects temperature/top_p and its thinking cannot be disabled, so
 * no sampling options are passed. @langchain/anthropic sends max_tokens 16384.
 */
export function createModel(modelName = process.env.DEEP_AGENTS_MODEL || DEFAULT_MODEL): BaseChatModel {
  return new ChatAnthropic({ model: modelName });
}

/** Dependencies of the agent; tests override them, production uses the defaults. */
export interface BuildAgentOptions {
  /** Chat model. Default: createModel(). */
  model?: BaseChatModel;
  /**
   * Saves conversation state per thread_id. Default: a new in-memory
   * MemorySaver, so history is lost when the process exits.
   */
  checkpointer?: MemorySaver;
  /** Client for the Rust world server. Default: RUST_ENGINE_URL or 127.0.0.1:3000. */
  engine?: EngineClient;
}

/**
 * Build the designer agent. The returned graph is invoked once per user turn;
 * pass `configurable.thread_id` to continue a conversation (see `ask`).
 */
export function buildAgent(options: BuildAgentOptions = {}) {
  return createDeepAgent({
    model: options.model ?? createModel(),
    systemPrompt: DESIGNER_PROMPT,
    tools: createEngineTools(options.engine ?? new EngineClient()),
    middleware: [todoListMiddleware()],
    checkpointer: options.checkpointer ?? new MemorySaver(),
  });
}

export type DesignerAgent = ReturnType<typeof buildAgent>;

/**
 * Send one user message on a thread and return the assistant's final text.
 * One call may run several model/tool steps; only the last message's text
 * blocks are returned (tool calls and thinking blocks are dropped).
 */
export async function ask(agent: DesignerAgent, threadId: string, message: string): Promise<string> {
  const result = await agent.invoke(
    { messages: [{ role: "user", content: message }] },
    { configurable: { thread_id: threadId } },
  );
  const last = result.messages.at(-1);
  if (!last) return "";
  const content = last.content;
  if (typeof content === "string") return content;
  return content
    .filter((block): block is { type: "text"; text: string } => block.type === "text")
    .map((block) => block.text)
    .join("");
}
