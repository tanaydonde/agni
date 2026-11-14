import { BaseChatModel } from "@langchain/core/language_models/chat_models";
import { AIMessage, type BaseMessage } from "@langchain/core/messages";
import type { ChatResult } from "@langchain/core/outputs";

/**
 * Offline stand-in for ChatAnthropic that replays scripted AI messages (text
 * or tool calls). It reports the ChatAnthropic class name so deepagents
 * resolves the same "anthropic" harness profile, and it records the tools the
 * agent binds and the messages it receives.
 */
export class ScriptedAnthropic extends BaseChatModel {
  boundToolNames: string[] = [];
  received: BaseMessage[][] = [];
  private step = 0;

  constructor(private readonly script: AIMessage[]) {
    super({});
  }

  override getName() {
    return "ChatAnthropic";
  }

  _llmType() {
    return "scripted";
  }

  override bindTools(tools: unknown[]) {
    this.boundToolNames = tools.map((t) => (t as { name: string }).name);
    return this as never;
  }

  async _generate(messages: BaseMessage[]): Promise<ChatResult> {
    this.received.push(messages);
    const message = this.script[Math.min(this.step++, this.script.length - 1)];
    return { generations: [{ message, text: typeof message.content === "string" ? message.content : "" }] };
  }
}

export const say = (text: string) => new AIMessage(text);

export const callTool = (name: string, args: Record<string, unknown> = {}, id = `call-${name}`) =>
  new AIMessage({ content: "", tool_calls: [{ id, name, args, type: "tool_call" }] });
