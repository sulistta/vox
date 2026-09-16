import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const core = join(packageRoot, "src", "main.ts");

function collect(proc: ReturnType<typeof spawn>): AsyncGenerator<Record<string, unknown>> {
  let buffer = "";
  const queue: Record<string, unknown>[] = [];
  let wake: (() => void) | undefined;
  proc.stdout!.on("data", (chunk) => {
    buffer += chunk.toString();
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) if (line) {
      queue.push(JSON.parse(line) as Record<string, unknown>);
      wake?.();
      wake = undefined;
    }
  });
  return (async function* () {
    while (true) {
      if (queue.length === 0) await new Promise<void>((resolve) => { wake = resolve; });
      yield queue.shift()!;
    }
  })();
}

test("core performs handshake and a fake textual turn", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], { cwd: packageRoot, stdio: ["pipe", "pipe", "pipe"] });
  const events = collect(proc);
  proc.stdin.write('{"type":"initialize"}\n');
  assert.equal((await events.next()).value?.type, "initialized");
  proc.stdin.write('{"type":"session.open","request_id":"req","session_id":"s"}\n');
  assert.equal((await events.next()).value?.type, "session.opened");
  proc.stdin.write('{"type":"turn.start","request_id":"req-2","session_id":"s","run_id":"r","content":"oi"}\n');
  const seen: string[] = [];
  for await (const event of events) {
    seen.push(String(event.type));
    if (event.type === "run.completed") break;
  }
  assert.equal(seen.includes("message.delta"), true);
  assert.equal(seen.at(-1), "run.completed");
  proc.stdin.end();
});

test("core exposes bounded model capabilities and rejects a mismatched model request", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], { cwd: packageRoot, stdio: ["pipe", "pipe", "pipe"] });
  const events = collect(proc);
  proc.stdin.write('{"type":"initialize"}\n');
  const initialized = (await events.next()).value!;
  assert.equal((initialized.capabilities as string[]).includes("context_chars:16384"), true);
  proc.stdin.write('{"type":"session.open","request_id":"req-model","session_id":"s-model"}\n');
  await events.next();
  proc.stdin.write(JSON.stringify({
    type: "turn.start",
    request_id: "req-model-turn",
    session_id: "s-model",
    run_id: "r-model",
    model_ref: "not-active",
    content: "oi",
  }) + "\n");
  const failed = (await events.next()).value!;
  assert.equal(failed.type, "run.failed");
  assert.equal(failed.error_code, "MODEL_MISMATCH");
  proc.stdin.end();
});

test("core pauses for the broker and resumes only with the matching tool result", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], {
    cwd: packageRoot,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const events = collect(proc);
  proc.stdin.write('{"type":"initialize"}\n');
  await events.next();
  proc.stdin.write('{"type":"session.open","request_id":"req","session_id":"s-tool"}\n');
  await events.next();
  proc.stdin.write(
    '{"type":"turn.start","request_id":"req-tool","session_id":"s-tool","run_id":"r-tool","content":"mostrar janelas"}\n',
  );
  let callId = "";
  let sawRequested = false;
  for await (const event of events) {
    if (event.type === "tool.execute.requested") {
      callId = String(event.call_id);
      sawRequested = true;
      assert.equal(event.tool, "desktop.list_windows");
      proc.stdin.write(
        JSON.stringify({
          type: "tool.result",
          run_id: "r-tool",
          call_id: callId,
          tool: "desktop.list_windows",
          status: "success",
          side_effect: "none",
          data: { windows: [], count: 0 },
          verification: { observed: true },
        }) + "\n",
      );
    }
    if (event.type === "run.completed") break;
  }
  assert.equal(sawRequested, true);
  assert.notEqual(callId, "");
  proc.stdin.end();
});

test("core cancellation reaches the provider and emits a terminal cancelled event", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], {
    cwd: packageRoot,
    stdio: ["pipe", "pipe", "pipe"],
  });
  const events = collect(proc);
  proc.stdin.write('{"type":"initialize"}\n');
  await events.next();
  proc.stdin.write('{"type":"session.open","request_id":"req","session_id":"s-cancel"}\n');
  await events.next();
  proc.stdin.write(
    JSON.stringify({
      type: "turn.start",
      request_id: "req-cancel",
      session_id: "s-cancel",
      run_id: "r-cancel",
      content: "x".repeat(12_000),
    }) + "\n",
  );
  let cancelled = false;
  for await (const event of events) {
    if (event.type === "message.delta") {
      proc.stdin.write(JSON.stringify({ type: "turn.cancel", request_id: "cancel", run_id: "r-cancel" }) + "\n");
    }
    if (event.type === "run.cancelled") {
      cancelled = true;
      break;
    }
    if (event.type === "run.failed") break;
  }
  assert.equal(cancelled, true);
  proc.stdin.end();
});
