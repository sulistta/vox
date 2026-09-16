import readline from "node:readline";
import { randomUUID } from "node:crypto";
import {
  PROTOCOL_VERSION,
  ProtocolError,
  encodeMessage,
  parseMessage,
  type IpcMessage,
  type IpcRequest,
  type JsonObject,
  type ToolResultMessage,
} from "@vox/protocol";
import { ProviderError, plannedTool, providerFromEnvironment } from "@vox/provider-adapters";
import { RunBudget, RunLimitError } from "./limits.js";

const provider = providerFromEnvironment();
const sessions = new Set<string>();
const runs = new Map<string, { controller: AbortController; sessionId: string }>();
const pendingTools = new Map<
  string,
  { runId: string; tool: string; resolve: (result: ToolResultMessage) => void }
>();

function send(message: IpcMessage): void {
  process.stdout.write(encodeMessage(message));
}

function log(message: string): void {
  process.stderr.write(`[vox-agent-core] ${message}\n`);
}

function state(session_id: string, run_id: string, value: "thinking" | "executing" | "cancelling"): void {
  send({ type: "state.changed", session_id, run_id, state: value });
}

async function waitForTool(
  runId: string,
  sessionId: string,
  plan: ReturnType<typeof plannedTool>,
  signal: AbortSignal,
): Promise<ToolResultMessage> {
  if (plan === undefined) throw new Error("no tool requested");
  const callId = randomUUID();
  state(sessionId, runId, "executing");
  send({ type: "tool.started", session_id: sessionId, run_id: runId, call_id: callId, tool: plan.tool, arguments: plan.arguments, effect: plan.effect });
  send({ type: "tool.execute.requested", session_id: sessionId, run_id: runId, call_id: callId, tool: plan.tool, arguments: plan.arguments, effect: plan.effect });
  return await new Promise<ToolResultMessage>((resolve, reject) => {
    const timer = setTimeout(() => {
      pendingTools.delete(callId);
      reject(new Error("tool timeout"));
    }, 10_000);
    const onAbort = () => {
      clearTimeout(timer);
      pendingTools.delete(callId);
      reject(new ProviderError("CANCELLED", "run cancelled while waiting for tool", false));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    pendingTools.set(callId, {
      runId,
      tool: plan.tool,
      resolve: (result) => {
      clearTimeout(timer);
      signal.removeEventListener("abort", onAbort);
      pendingTools.delete(callId);
      resolve(result);
      },
    });
  });
}

async function runTurn(request: Extract<IpcRequest, { type: "turn.start" }>): Promise<void> {
  if (runs.size > 0) {
    send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: "RUN_BUSY", error: "another run is active", retryable: true });
    return;
  }
  const controller = new AbortController();
  const budget = new RunBudget();
  runs.set(request.run_id, { controller, sessionId: request.session_id });
  state(request.session_id, request.run_id, "thinking");
  let seq = 0;
  let content = "";
  let timeout: NodeJS.Timeout | undefined;
  try {
    budget.validatePrompt(request.content);
    timeout = setTimeout(() => controller.abort("RUN_TIMEOUT"), budget.maxDurationMs);
    const plan = plannedTool(request.content);
    if (plan) {
      budget.consumeStep();
      const result = await waitForTool(request.run_id, request.session_id, plan, controller.signal);
      send({ type: "tool.completed", session_id: request.session_id, run_id: request.run_id, call_id: result.call_id, tool: plan.tool, status: result.status, side_effect: result.side_effect ?? "none", data: result.data, error: result.error, verification: result.verification });
      budget.observe(JSON.stringify({ tool: plan.tool, status: result.status, data: result.data }));
      if (result.status !== "success") {
        throw new ProviderError(
          result.status === "unknown" ? "UNKNOWN_EFFECT" : result.error_code ?? "TOOL_ERROR",
          result.error ?? "tool failed",
          false,
        );
      }
      state(request.session_id, request.run_id, "thinking");
    }
    for await (const delta of provider.stream(request.content, controller.signal)) {
      content += delta;
      budget.validateOutput(content);
      seq += 1;
      send({ type: "message.delta", session_id: request.session_id, run_id: request.run_id, seq, delta });
    }
    send({ type: "run.completed", session_id: request.session_id, run_id: request.run_id, seq: Math.max(seq, 1), content });
  } catch (error) {
    if (error instanceof RunLimitError) {
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: error.code, error: error.message, retryable: false });
    } else if (controller.signal.reason === "RUN_TIMEOUT") {
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: "RUN_TIMEOUT", error: "run exceeded its time budget", retryable: false });
    } else if (controller.signal.aborted || (error instanceof ProviderError && error.code === "CANCELLED")) {
      send({ type: "run.cancelled", session_id: request.session_id, run_id: request.run_id, reason: "cancelled by user", completed_effects: 0 });
    } else if (error instanceof ProviderError) {
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: error.code, error: error.message, retryable: error.retryable });
    } else {
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: "CORE_ERROR", error: error instanceof Error ? error.message : "agent core failed", retryable: false });
    }
  } finally {
    if (timeout) clearTimeout(timeout);
    runs.delete(request.run_id);
  }
}

function handle(message: IpcMessage): void {
  if (message.type === "initialize") {
    send({ type: "initialized", protocol: PROTOCOL_VERSION, runtime: process.version, build: "vox-agent-core-dev", capabilities: ["text", "stream", "cancel", "tool.request", provider.id] });
  } else if (message.type === "session.open") {
    sessions.add(message.session_id);
    send({ type: "session.opened", request_id: message.request_id, session_id: message.session_id });
  } else if (message.type === "turn.start") {
    if (!sessions.has(message.session_id)) {
      send({ type: "run.failed", session_id: message.session_id, run_id: message.run_id, error_code: "SESSION_NOT_OPEN", error: "session must be opened before a turn", retryable: false });
      return;
    }
    void runTurn(message);
  } else if (message.type === "turn.cancel") {
    const run = runs.get(message.run_id);
    run?.controller.abort();
    if (run) {
      send({ type: "state.changed", session_id: run.sessionId, run_id: message.run_id, state: "cancelling" });
    }
  } else if (message.type === "heartbeat") {
    send({ type: "heartbeat", request_id: message.request_id, runtime: process.version, healthy: true });
  } else if (message.type === "tool.result") {
    const callId = message.call_id;
    const pending = callId ? pendingTools.get(callId) : undefined;
    if (pending && pending.runId === message.run_id && pending.tool === message.tool) {
      pending.resolve(message);
    } else if (callId) {
      send({ type: "error", error_code: "TOOL_RESULT_MISMATCH", error: "tool result does not match a pending call", retryable: false });
    }
  } else if (message.type === "shutdown") {
    for (const { controller } of runs.values()) controller.abort();
    process.exitCode = 0;
  }
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  try {
    handle(parseMessage(line));
  } catch (error) {
    if (error instanceof ProtocolError) send({ type: "error", error_code: error.code, error: error.message, retryable: false });
    else log(error instanceof Error ? error.message : "unknown protocol failure");
  }
});

input.on("close", () => {
  for (const { controller } of runs.values()) controller.abort();
});
