import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const entry = join(root, "packages", "agent-core", "dist", "main.js");
const mode = process.env.VOX_PROVIDER_SMOKE_MODE === "cancel" ? "cancel" : "conversation";
const requiredEnvironment = ["VOX_PROVIDER_BASE_URL", "VOX_PROVIDER_MODEL", "VOX_PROVIDER_API_KEY"];

function finish(result) {
  process.stdout.write(`${JSON.stringify(result)}\n`);
  process.exitCode = result.status === "passed" ? 0 : 1;
}

if (!existsSync(entry)) {
  finish({ status: "failed", stage: "setup", reason: "agent core is not built" });
} else if (requiredEnvironment.some((name) => !process.env[name]?.trim())) {
  finish({ status: "failed", stage: "setup", reason: "provider environment is incomplete" });
} else {
  const child = spawn(process.execPath, [entry], {
    cwd: root,
    env: { ...process.env, VOX_PROVIDER: "openai-compatible" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let buffer = "";
  const events = [];
  let wake;
  let stderrChunks = 0;
  let childFailedToSpawn = false;

  child.stdout.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) {
      if (!line) continue;
      try {
        events.push(JSON.parse(line));
      } catch {
        events.push({ type: "invalid.core.output" });
      }
      wake?.();
      wake = undefined;
    }
  });
  // The core's stderr can include provider failures. Count it for diagnosis,
  // but never mirror it because external services may echo sensitive input.
  child.stderr.on("data", () => { stderrChunks += 1; });
  child.on("error", () => {
    childFailedToSpawn = true;
    wake?.();
    wake = undefined;
  });

  function send(message) {
    if (!child.stdin.destroyed) child.stdin.write(`${JSON.stringify(message)}\n`);
  }

  async function nextEvent(timeoutMs) {
    if (events.length) return events.shift();
    return await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("timeout")), timeoutMs);
      wake = () => {
        clearTimeout(timeout);
        resolve(events.shift());
      };
    });
  }

  async function expect(type, timeoutMs = 10_000) {
    const event = await nextEvent(timeoutMs);
    if (event?.type !== type) throw new Error("unexpected core event");
    return event;
  }

  const startedAt = Date.now();
  const eventTypes = [];
  let auditRedacted = false;
  let auditExposedMarker = false;
  let toolRequestsBlocked = 0;
  let responseChars = 0;
  let terminalType = "none";
  let errorCode = null;
  let stage = "startup";
  let cancelled = false;

  try {
    send({ type: "initialize", request_id: "provider-smoke-init" });
    const initialized = await expect("initialized");
    eventTypes.push(initialized.type);
    if (!Array.isArray(initialized.capabilities) || !initialized.capabilities.includes("openai-compatible")) {
      throw new Error("provider was not selected");
    }

    stage = "session";
    send({ type: "session.open", request_id: "provider-smoke-open", session_id: "provider-smoke-session" });
    const opened = await expect("session.opened");
    eventTypes.push(opened.type);

    stage = "turn";
    send({
      type: "turn.start",
      request_id: "provider-smoke-turn",
      session_id: "provider-smoke-session",
      run_id: "provider-smoke-run",
      model_ref: process.env.VOX_PROVIDER_MODEL,
      content: "Responda apenas com uma saudação curta em português. Não use ferramentas. sk-or-v1-0123456789abcdef0123456789abcdef",
    });

    const deadline = Date.now() + 60_000;
    while (Date.now() < deadline) {
      const event = await nextEvent(Math.max(1, deadline - Date.now()));
      const eventType = typeof event?.type === "string" ? event.type : "invalid.core.output";
      eventTypes.push(eventType);

      if (eventType === "model.requested") {
        const visibleMessages = JSON.stringify(event.messages ?? []);
        auditRedacted ||= event.redacted === true && visibleMessages.includes("[REDACTED]");
        auditExposedMarker ||= visibleMessages.includes("sk-or-v1-0123456789abcdef0123456789abcdef");
        if (mode === "cancel" && !cancelled) {
          cancelled = true;
          send({ type: "turn.cancel", request_id: "provider-smoke-cancel", run_id: "provider-smoke-run" });
        }
      } else if (eventType === "tool.execute.requested") {
        // This smoke never grants a tool result that could cause a real effect.
        toolRequestsBlocked += 1;
        if (toolRequestsBlocked > 2) throw new Error("model requested too many tools");
        send({
          type: "tool.result",
          run_id: "provider-smoke-run",
          call_id: event.call_id,
          tool: event.tool,
          status: "error",
          side_effect: "none",
          error_code: "SMOKE_TOOL_BLOCKED",
          error: "No external tool is allowed during the provider smoke test.",
        });
      } else if (eventType === "run.completed") {
        terminalType = eventType;
        responseChars = typeof event.content === "string" ? event.content.length : 0;
        break;
      } else if (eventType === "run.failed" || eventType === "run.cancelled") {
        terminalType = eventType;
        errorCode = typeof event.error_code === "string" ? event.error_code : null;
        break;
      }
    }
  } catch {
    stage = stage === "startup" ? "timeout-or-startup" : stage;
  } finally {
    child.stdin.end();
    child.kill();
  }

  const passed = mode === "cancel"
    ? terminalType === "run.cancelled" && auditRedacted && !auditExposedMarker && toolRequestsBlocked === 0
    : terminalType === "run.completed" && responseChars > 0 && auditRedacted && !auditExposedMarker && toolRequestsBlocked === 0;
  finish({
    status: passed ? "passed" : "failed",
    stage: passed ? "complete" : stage,
    mode,
    provider: "openai-compatible",
    model_ref: process.env.VOX_PROVIDER_MODEL,
    event_types: [...new Set(eventTypes)],
    terminal_type: terminalType,
    error_code: errorCode,
    response_chars: responseChars,
    audit_redacted: auditRedacted,
    audit_marker_exposed: auditExposedMarker,
    tool_requests_blocked: toolRequestsBlocked,
    core_stderr_chunks_observed: stderrChunks,
    core_spawn_failed: childFailedToSpawn,
    elapsed_ms: Date.now() - startedAt,
  });
}
