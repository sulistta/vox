import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
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

test("invalid provider configuration becomes a structured turn failure instead of ending the core", async () => {
  const proc = spawn(process.execPath, ["--import", "tsx", core], {
    cwd: packageRoot,
    env: {
      ...process.env,
      VOX_PROVIDER: "openai-compatible",
      VOX_PROVIDER_BASE_URL: "not a URL",
      VOX_PROVIDER_MODEL: "test-model",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const events = collect(proc);
  try {
    proc.stdin.write('{"type":"initialize"}\n');
    const initialized = (await events.next()).value!;
    assert.equal(initialized.type, "initialized");
    assert.equal((initialized.capabilities as string[]).includes("unavailable-provider"), true);
    proc.stdin.write('{"type":"session.open","request_id":"invalid-config-open","session_id":"invalid-config"}\n');
    assert.equal((await events.next()).value?.type, "session.opened");
    proc.stdin.write('{"type":"turn.start","request_id":"invalid-config-turn","session_id":"invalid-config","run_id":"invalid-config-run","content":"oi"}\n');

    let failed: Record<string, unknown> | undefined;
    while (!failed) {
      const event = (await events.next()).value!;
      if (event.type === "run.failed") {
        failed = event;
      }
    }
    assert.equal(failed?.error_code, "CONFIG_ERROR");

    proc.stdin.write('{"type":"heartbeat","request_id":"invalid-config-heartbeat"}\n');
    let heartbeat: Record<string, unknown> | undefined;
    while (!heartbeat) {
      const event = (await events.next()).value!;
      if (event.type === "heartbeat") {
        heartbeat = event;
      }
    }
    assert.equal(heartbeat.healthy, true);
  } finally {
    proc.stdin.end();
  }
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
    env: { ...process.env, VOX_PROVIDER: "fake-tools" },
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
          error_code: null,
          error: null,
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

test("the model chooses the tool, receives its result in a second request, and activity mirrors redacted provider input", async () => {
  const submittedProviderKey = "sk-or-v1-0123456789abcdef0123456789abcdef";
  const echoedProviderKey = "sk-or-v1-fedcba9876543210fedcba9876543210";
  const submittedPortugueseSecrets = "senha=senha-local segredo:segredo-local chave=chave-local credenciais:credencial-local";
  const requests: Array<{ messages: Array<{ role: string; content: string }> }> = [];
  const server = createServer((request, response) => {
    let body = "";
    request.setEncoding("utf8");
    request.on("data", (chunk: string) => { body += chunk; });
    request.on("end", () => {
      const parsed = JSON.parse(body) as { messages?: Array<{ role: string; content: string }> };
      const messages = parsed.messages ?? [];
      requests.push({ messages });
      const latestUser = [...messages].reverse().find((message) => message.role === "user")?.content ?? "";
      const decision = latestUser.startsWith("TOOL_RESULT\n")
        ? { type: "final", content: `A janela Sentinel está visível. ${echoedProviderKey}` }
        : { type: "tool_call", tool: "desktop.list_windows", arguments: {} };
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify(decision) } }] }));
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const proc = spawn(process.execPath, ["--import", "tsx", core], {
    cwd: packageRoot,
    env: {
      ...process.env,
      VOX_PROVIDER: "openai-compatible",
      VOX_PROVIDER_BASE_URL: `http://127.0.0.1:${address.port}`,
      VOX_PROVIDER_MODEL: "test-model",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  try {
    const events = collect(proc);
    proc.stdin.write('{"type":"initialize"}\n');
    await events.next();
    proc.stdin.write('{"type":"session.open","request_id":"open-model","session_id":"model-session"}\n');
    await events.next();
    proc.stdin.write(JSON.stringify({
      type: "turn.start",
      request_id: "model-turn",
      session_id: "model-session",
      run_id: "model-run",
      content: `mostre a janela API_KEY=very-secret ${submittedPortugueseSecrets} ${submittedProviderKey}`,
      context: [{ role: "assistant", content: "mensagem anterior" }],
    }) + "\n");

    const activity: Array<{ messages: Array<{ role: string; content: string }> }> = [];
    let toolRequested = false;
    let completed = "";
    let streamed = "";
    for await (const event of events) {
      if (event.type === "model.requested") {
        activity.push({ messages: event.messages as Array<{ role: string; content: string }> });
      }
      if (event.type === "tool.execute.requested") {
        toolRequested = true;
        assert.equal(event.tool, "desktop.list_windows");
        proc.stdin.write(JSON.stringify({
          type: "tool.result",
          run_id: "model-run",
          call_id: event.call_id,
          tool: "desktop.list_windows",
          status: "success",
          side_effect: "none",
          data: {
            windows: [{ name: "SENTINEL_TOOL_OUTPUT" }],
            token: "tool-secret",
            senha: "tool-password-portuguese",
            segredo: "tool-secret-portuguese",
          },
          verification: { observed: true },
        }) + "\n");
      }
      if (event.type === "message.delta") {
        streamed += String(event.delta ?? "");
      }
      if (event.type === "run.completed") {
        completed = String(event.content);
        break;
      }
      if (event.type === "run.failed") {
        assert.fail(`model loop failed: ${JSON.stringify(event)}`);
      }
    }

    assert.equal(toolRequested, true);
    assert.equal(completed.includes(echoedProviderKey), false);
    assert.equal(streamed.includes(echoedProviderKey), false);
    assert.equal(completed.includes("[REDACTED]"), true);
    assert.equal(requests.length, 2);
    assert.equal(activity.length, 2);
    assert.deepEqual(activity.map((entry) => entry.messages), requests.map((entry) => entry.messages));
    const firstRequest = JSON.stringify(requests[0]?.messages);
    const secondRequest = JSON.stringify(requests[1]?.messages);
    assert.equal(firstRequest.includes("very-secret"), false);
    assert.equal(firstRequest.includes(submittedProviderKey), false);
    assert.equal(firstRequest.includes("senha-local"), false);
    assert.equal(firstRequest.includes("segredo-local"), false);
    assert.equal(firstRequest.includes("chave-local"), false);
    assert.equal(firstRequest.includes("credencial-local"), false);
    assert.equal(firstRequest.includes("[REDACTED]"), true);
    assert.equal(secondRequest.includes("SENTINEL_TOOL_OUTPUT"), true);
    assert.equal(secondRequest.includes("tool-secret"), false);
    assert.equal(secondRequest.includes("tool-password-portuguese"), false);
    assert.equal(secondRequest.includes("tool-secret-portuguese"), false);
  } finally {
    proc.stdin.end();
    await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});

test("core refuses a model tool call that carries a standalone key or Portuguese credential", async () => {
  const providerKey = "sk-or-v1-0123456789abcdef0123456789abcdef";
  const portugueseSecret = "senha=senha-em-argumento";
  const server = createServer((_request, response) => {
    const decision = {
      type: "tool_call",
      tool: "files.write",
      arguments: { path: "safe.txt", content: `${providerKey} ${portugueseSecret}` },
    };
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify(decision) } }] }));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const proc = spawn(process.execPath, ["--import", "tsx", core], {
    cwd: packageRoot,
    env: {
      ...process.env,
      VOX_PROVIDER: "openai-compatible",
      VOX_PROVIDER_BASE_URL: `http://127.0.0.1:${address.port}`,
      VOX_PROVIDER_MODEL: "test-model",
    },
    stdio: ["pipe", "pipe", "pipe"],
  });
  try {
    const events = collect(proc);
    proc.stdin.write('{"type":"initialize"}\n');
    await events.next();
    proc.stdin.write('{"type":"session.open","request_id":"open-sensitive-call","session_id":"s-sensitive-call"}\n');
    await events.next();
    proc.stdin.write(JSON.stringify({
      type: "turn.start",
      request_id: "turn-sensitive-call",
      session_id: "s-sensitive-call",
      run_id: "r-sensitive-call",
      content: "crie um arquivo",
    }) + "\n");

    let toolRequested = false;
    let failed: Record<string, unknown> | undefined;
    for await (const event of events) {
      if (event.type === "tool.execute.requested") toolRequested = true;
      if (event.type === "run.failed") {
        failed = event;
        break;
      }
      if (event.type === "run.completed" || event.type === "run.cancelled") break;
    }
    assert.equal(toolRequested, false);
    assert.equal(failed?.error_code, "SENSITIVE_TOOL_ARGUMENT");
    assert.equal(String(failed?.error ?? "").includes(providerKey), false);
    assert.equal(String(failed?.error ?? "").includes(portugueseSecret), false);
  } finally {
    proc.stdin.end();
    await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
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
    if (event.type === "model.requested") {
      proc.stdin.write(JSON.stringify({ type: "turn.cancel", request_id: "cancel", run_id: "r-cancel" }) + "\n");
    }
    if (event.type === "run.cancelled") {
      cancelled = true;
      break;
    }
    if (event.type === "run.failed" || event.type === "run.completed") break;
  }
  assert.equal(cancelled, true);
  proc.stdin.end();
});
