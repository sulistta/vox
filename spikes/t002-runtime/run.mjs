import { spawn } from "node:child_process";
import { performance } from "node:perf_hooks";
import { readFileSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = dirname(fileURLToPath(import.meta.url));
const child = join(root, "child.mjs");

function waitFor(stream, predicate, timeoutMs = 2000) {
  return new Promise((resolve, reject) => {
    let buffer = "";
    const timer = setTimeout(() => reject(new Error("timeout")), timeoutMs);
    const onData = (chunk) => {
      buffer += chunk;
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        if (!line) continue;
        const message = JSON.parse(line);
        if (predicate(message)) {
          clearTimeout(timer);
          stream.off("data", onData);
          resolve(message);
          return;
        }
      }
    };
    stream.on("data", onData);
    stream.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

async function processRun(mode = "normal") {
  const started = performance.now();
  const proc = spawn(process.execPath, [child], { stdio: ["pipe", "pipe", "pipe"] });
  proc.stdin.write(`${JSON.stringify({ type: "initialize" })}\n`);
  await waitFor(proc.stdout, (message) => message.type === "initialized");
  const initializedMs = performance.now() - started;
  const runId = `run-${Math.random().toString(16).slice(2)}`;
  proc.stdin.write(`${JSON.stringify({ type: "turn.start", run_id: runId, mode })}\n`);
  const firstEvent = await waitFor(proc.stdout, (message) => message.run_id === runId);
  const firstEventMs = performance.now() - started;
  if (mode === "cancel") {
    proc.stdin.write(`${JSON.stringify({ type: "turn.cancel", run_id: runId })}\n`);
  }
  const terminal = await waitFor(proc.stdout, (message) =>
    message.run_id === runId && ["run.completed", "run.cancelled"].includes(message.type),
  );
  const terminalMs = performance.now() - started;
  proc.kill();
  return { initializedMs, firstEvent: firstEvent.type, firstEventMs, terminal: terminal.type, terminalMs };
}

async function processCrash() {
  const started = performance.now();
  const proc = spawn(process.execPath, [child], { stdio: ["pipe", "pipe", "pipe"] });
  proc.stdin.write(`${JSON.stringify({ type: "initialize" })}\n`);
  await waitFor(proc.stdout, (message) => message.type === "initialized");
  proc.stdin.write(`${JSON.stringify({ type: "turn.start", run_id: "crash-run", mode: "crash" })}\n`);
  const exit = await new Promise((resolve) => proc.once("exit", (code, signal) => resolve({ code, signal })));
  return { exit, elapsedMs: performance.now() - started };
}

async function embeddedRun(mode = "normal") {
  const started = performance.now();
  const chunks = ["Vox ", "processou ", "o turno ", "com segurança."];
  const firstEvent = "run.started";
  await new Promise((resolve) => setTimeout(resolve, 5));
  const terminal = mode === "cancel" ? "run.cancelled" : "run.completed";
  return { initializedMs: 0, firstEvent, firstEventMs: performance.now() - started, terminal, terminalMs: performance.now() - started, chunks: chunks.length };
}

const normal = await Promise.all(Array.from({ length: 5 }, () => processRun("normal")));
const cancelled = await processRun("cancel");
const crashed = await processCrash();
const embedded = await Promise.all(Array.from({ length: 5 }, () => embeddedRun("normal")));

const report = {
  node: process.version,
  childBytes: statSync(child).size,
  sourceBytes: readFileSync(child).byteLength,
  process: { normal, cancelled, crashed },
  embedded,
  interpretation: {
    processIsolation: "crash/cancel are observable as protocol events; startup has a measurable cost",
    embedding: "no process boundary; crash isolation is unavailable unless the host isolates the runtime",
  },
};
process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
