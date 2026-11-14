/**
 * Contract tests against the real Rust world server (crates/server), using its
 * built-in demo room. Build it first: `cargo build -p tflop-server` (repo root).
 * Skipped when the binary is missing.
 */
import { type ChildProcess, spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { buildAgent } from "../src/agent.js";
import { EngineClient } from "../src/engine.js";
import { createEngineTools, summarizeScene } from "../src/tools.js";
import { callTool, say, ScriptedAnthropic } from "./helpers.js";

const binary = fileURLToPath(new URL("../../../target/debug/tflop-server", import.meta.url));
const demoScene = fileURLToPath(new URL("../../../crates/server/fixtures/demo-room.json", import.meta.url));
const port = 3100 + Math.floor(Math.random() * 800);
const engine = new EngineClient(`http://127.0.0.1:${port}`);

let server: ChildProcess | undefined;

beforeAll(async () => {
  if (!existsSync(binary)) return;
  // Pin the demo room explicitly: the server also reads the repo-root .env, so
  // a developer's TFLOP_SCENE_PATH there must not change what we test against.
  server = spawn(binary, ["--addr", `127.0.0.1:${port}`, "--scene", demoScene], { stdio: "ignore" });
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`http://127.0.0.1:${port}/health`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("tflop-server did not start");
});

afterAll(() => {
  server?.kill();
});

/** Look up an engine tool by name, typed loosely so tests can invoke any of them. */
const tool = (name: string) => {
  const found = createEngineTools(engine).find((t) => t.name === name);
  if (!found) throw new Error(`no tool ${name}`);
  return found as unknown as { invoke(input: unknown): Promise<unknown> };
};

describe.skipIf(!existsSync(binary))("world server contract", () => {
  it("get_scene returns a compact summary of the demo room", async () => {
    const result = await engine.getScene();
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    const summary = summarizeScene(result.value.revision, result.value.scene);
    expect(summary.revision).toBe(0);

    const objects = Object.fromEntries(summary.floors[0].objects.map((o) => [o.object_id, o]));
    expect(objects["sofa-1"]).toMatchObject({ x: 7.3, y: 3, yaw_deg: 90, category: "sofa" });
    expect(objects["chair-2"].yaw_deg).toBe(180);

    const door = summary.floors[0].rooms[0].openings.find((o) => o.id === "door-1");
    expect(door).toMatchObject({ kind: "door", from: { x: 3.5, y: 0 }, to: { x: 4.5, y: 0 } });
  });

  it("apply_edits sends every edit type in a shape the server accepts", async () => {
    // A 400 here would mean our zod/TS shapes do not match world-core's serde
    // types; 501 means the server parsed the request and reached the stub.
    const output = await tool("apply_edits").invoke({
      expected_revision: 0,
      edits: [
        { type: "add_object", object_id: "chair-9", asset_id: "chair-basic", floor_id: "floor-1", x: 1, y: 1, yaw_deg: 45 },
        { type: "move_object", object_id: "chair-1", x: 2, y: 2 },
        { type: "rotate_object", object_id: "table-1", yaw_deg: 90 },
        { type: "remove_object", object_id: "chair-2" },
      ],
    });
    expect(JSON.parse(output as string)).toEqual({
      error: { code: "not_implemented", operation: "apply_edits", status: 501 },
    });
  });

  it("validate_layout surfaces the engine error instead of throwing", async () => {
    const output = await tool("validate_layout").invoke({});
    expect(JSON.parse(output as string).error).toMatchObject({ code: "not_implemented", status: 501 });
  });

  it("runs get_scene inside the agent loop", async () => {
    const model = new ScriptedAnthropic([callTool("get_scene"), say("The room has a sofa.")]);
    const result = await buildAgent({ model, engine }).invoke(
      { messages: [{ role: "user", content: "What is in the room?" }] },
      { configurable: { thread_id: "loop" } },
    );
    const toolMessage = result.messages.find((m) => m.getType() === "tool");
    expect(String(toolMessage?.content)).toContain('"scene_id":"demo-room"');
    expect(result.messages.at(-1)?.content).toBe("The room has a sofa.");
  });
});

describe("engine client without a server", () => {
  it("reports engine_unreachable instead of throwing", async () => {
    const result = await new EngineClient("http://127.0.0.1:9").getScene();
    expect(result).toMatchObject({ ok: false, error: { code: "engine_unreachable" } });
  });
});
