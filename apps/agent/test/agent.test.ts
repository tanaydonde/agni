/**
 * Offline tests of agent construction with a scripted fake model: thread
 * history, the system prompt, and the exact set of tools bound to the model.
 * No API key or world server needed.
 */
import { describe, expect, it } from "vitest";

import { ask, buildAgent } from "../src/agent.js";
import { EngineClient } from "../src/engine.js";
import { say, ScriptedAnthropic } from "./helpers.js";

// Nothing listens on port 9; these tests never reach the engine.
const offlineEngine = new EngineClient("http://127.0.0.1:9");

describe("designer agent", () => {
  it("answers a question and keeps the conversation on the thread", async () => {
    const agent = buildAgent({ model: new ScriptedAnthropic([say("First."), say("Second.")]), engine: offlineEngine });

    expect(await ask(agent, "t1", "How wide should an aisle be?")).toBe("First.");
    const second = await agent.invoke(
      { messages: [{ role: "user", content: "And for wheelchairs?" }] },
      { configurable: { thread_id: "t1" } },
    );
    expect(second.messages.map((m) => m.getType())).toEqual(["human", "ai", "human", "ai"]);
  });

  it("sends our system prompt without advertising subagents", async () => {
    const model = new ScriptedAnthropic([say("ok")]);
    await ask(buildAgent({ model, engine: offlineEngine }), "t2", "hi");
    const system = model.received[0].find((m) => m.getType() === "system");
    const text = JSON.stringify(system?.content ?? "");
    expect(text).toContain("tflop");
    expect(text).not.toMatch(/`task`|subagent/i);
  });

  it("binds exactly the engine, planning, and virtual-file tools", async () => {
    const model = new ScriptedAnthropic([say("ok")]);
    await ask(buildAgent({ model, engine: offlineEngine }), "t3", "hi");
    expect([...model.boundToolNames].sort()).toEqual(
      [
        "apply_edits",
        "delete", // virtual-file tool from deepagents' filesystem middleware
        "edit_file",
        "get_scene",
        "glob",
        "grep",
        "list_assets",
        "ls",
        "read_file",
        "validate_layout",
        "write_file",
        "write_todos",
      ].sort(),
    );
  });
});
