import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { parseMessage } from "@vox/protocol";

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const core = join(packageRoot, "src", "main.ts");

function events(process: ReturnType<typeof spawn>): AsyncGenerator<Record<string, unknown>> {
  let buffer = ""; const queue: Record<string, unknown>[] = []; let wake: (() => void) | undefined;
  process.stdout!.on("data", (chunk) => {
    buffer += String(chunk);
    const lines = buffer.split("\n"); buffer = lines.pop() ?? "";
    for (const line of lines) if (line) { queue.push(JSON.parse(line) as Record<string, unknown>); wake?.(); wake = undefined; }
  });
  return (async function* () { for (;;) { if (!queue.length) await new Promise<void>((resolve, reject) => { const timer = setTimeout(() => reject(new Error("core timeout")), 5_000); wake = () => { clearTimeout(timer); resolve(); }; }); yield queue.shift()!; } })();
}

test("Jev selects every step while text produces only TaskSpec, candidates and final text", async () => {
  let textRequests = 0; let agentRequests = 0;
  const api = createServer((request, response) => {
    let body = ""; request.on("data", (chunk) => { body += chunk; }); request.on("end", () => {
      response.setHeader("content-type", "application/json");
      if (request.url === "/agent") {
        agentRequests += 1;
        const choices = Object.keys((JSON.parse(body) as { questions: { next_step: { criteria: Record<string, string> } } }).questions.next_step.criteria);
        const selected = agentRequests === 1 ? "inspect" : "finish";
        const probabilities = Object.fromEntries(choices.map((id) => [id, id === selected ? 0.96 : 0.04 / (choices.length - 1)]));
        const answers = agentRequests === 1
          ? { next_step: { type: "choice", choice: selected, confidence: 0.9, probabilities }, evidence_quality: { type: "score", score: 1.88, confidence: 0.9, probabilities: { "0": 0.02, "1": 0.08, "2": 0.9 } }, goal_verified: { type: "noul", noul: 0.1 } }
          : { next_step: { type: "choice", choice: selected, confidence: 0.97, probabilities }, evidence_quality: { type: "score", score: 1.96, confidence: 0.97, probabilities: { "0": 0.01, "1": 0.02, "2": 0.97 } }, goal_verified: { type: "noul", noul: 0.95 } };
        response.end(JSON.stringify({ model: "jev-resolved", answers })); return;
      }
      textRequests += 1;
      const content = textRequests === 1
        ? JSON.stringify({ objective: "listar janelas", app_hints: [], targets: [], expected_result: "lista de janelas observada" })
        : textRequests <= 3
          ? JSON.stringify({ candidates: [{ id: "inspect", tool: "desktop.list_windows", arguments: {}, label: "listar janelas", verification: "lista observada" }] })
          : "A lista de janelas foi observada.";
      response.end(JSON.stringify({ choices: [{ message: { content } }] }));
    });
  });
  await new Promise<void>((resolve) => api.listen(0, "127.0.0.1", resolve));
  const address = api.address(); assert.ok(address && typeof address !== "string");
  const proc = spawn(process.execPath, ["--import", "tsx", core], { cwd: packageRoot, stdio: ["pipe", "pipe", "pipe"], env: { ...process.env, VOX_TEXT_BASE_URL: `http://127.0.0.1:${address.port}`, VOX_TEXT_MODEL: "text", VOX_TEXT_API_KEY: "text-key", VOX_AGENT_BASE_URL: `http://127.0.0.1:${address.port}/agent`, VOX_AGENT_MODEL: "jev-latest", VOX_AGENT_API_KEY: "agent-key" } });
  try {
    const output = events(proc);
    proc.stdin.write('{"type":"initialize"}\n'); assert.equal((await output.next()).value?.type, "initialized");
    proc.stdin.write('{"type":"session.open","request_id":"open","session_id":"s"}\n'); assert.equal((await output.next()).value?.type, "session.opened");
    proc.stdin.write('{"type":"turn.start","request_id":"turn","session_id":"s","run_id":"r","task_grant_id":"grant","content":"liste"}\n');
    let sawText = false; let sawJev = false; let sawCompletedTool = false; let terminal: Record<string, unknown> | undefined;
    for await (const event of output) {
      if (event.type === "model.requested") { sawText ||= event.provider === "openai-compatible-text"; sawJev ||= event.provider === "typesafe-jev"; }
      if (event.type === "tool.execute.requested") proc.stdin.write(`${JSON.stringify({ type: "tool.result", run_id: "r", call_id: event.call_id, tool: event.tool, status: "success", side_effect: "none", data: { windows: [] }, verification: { observed: true } })}\n`);
      if (event.type === "tool.completed") {
        assert.equal(typeof event.call_id, "string");
        assert.notEqual(event.call_id, "");
        assert.doesNotThrow(() => parseMessage(JSON.stringify(event)));
        sawCompletedTool = true;
      }
      if (event.type === "run.completed" || event.type === "run.failed") { terminal = event; break; }
    }
    assert.equal(sawText, true); assert.equal(sawJev, true); assert.equal(terminal?.type, "run.completed", JSON.stringify(terminal));
    assert.equal(sawCompletedTool, true);
    assert.equal(agentRequests, 2); assert.ok(textRequests >= 3);
  } finally { proc.kill(); await new Promise<void>((resolve) => api.close(() => resolve())); }
});

test("missing mandatory providers becomes a structured run failure", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], { cwd: packageRoot, stdio: ["pipe", "pipe", "pipe"], env: { PATH: process.env.PATH ?? "" } });
  try {
    const output = events(proc);
    proc.stdin.write('{"type":"initialize"}\n'); await output.next();
    proc.stdin.write('{"type":"session.open","request_id":"open","session_id":"s"}\n'); await output.next();
    proc.stdin.write('{"type":"turn.start","request_id":"turn","session_id":"s","run_id":"r","task_grant_id":"grant","content":"oi"}\n');
    for await (const event of output) if (event.type === "run.failed") { assert.equal(event.error_code, "CONFIG_ERROR"); break; }
  } finally { proc.kill(); }
});
