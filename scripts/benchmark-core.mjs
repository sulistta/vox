import { readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const entry = join(root, "packages", "agent-core", "dist", "main.js");
const samples = Number.parseInt(process.env.VOX_BENCHMARK_SAMPLES ?? "5", 10);
if (!readFileSync(entry, "utf8")) throw new Error(`core entry is empty: ${entry}`);

function rssBytes(pid) {
  try {
    const status = readFileSync(`/proc/${pid}/status`, "utf8");
    const match = /^VmRSS:\s+(\d+)\s+kB$/mu.exec(status);
    return match ? Number(match[1]) * 1024 : null;
  } catch {
    return null;
  }
}

function percentile(values, fraction) {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor((sorted.length - 1) * fraction))];
}

async function measure(index) {
  const child = spawn(process.execPath, [entry], {
    cwd: root,
    env: { ...process.env, VOX_PROVIDER: "fake" },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let buffer = "";
  const events = [];
  child.stdout.on("data", (chunk) => {
    buffer += chunk.toString();
    const lines = buffer.split("\n");
    buffer = lines.pop() ?? "";
    for (const line of lines) if (line) events.push(JSON.parse(line));
  });
  const waitFor = async (type, timeoutMs = 3_000) => {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const found = events.find((event) => event.type === type);
      if (found) return found;
      await new Promise((resolve) => setTimeout(resolve, 2));
    }
    throw new Error(`benchmark sample ${index} timed out waiting for ${type}`);
  };
  const startedAt = performance.now();
  child.stdin.write(JSON.stringify({ type: "initialize" }) + "\n");
  await waitFor("initialized");
  const initializedMs = performance.now() - startedAt;
  child.stdin.write(JSON.stringify({ type: "session.open", request_id: `bench-open-${index}`, session_id: `bench-${index}` }) + "\n");
  await waitFor("session.opened");
  const turnStartedAt = performance.now();
  child.stdin.write(JSON.stringify({
    type: "turn.start",
    request_id: `bench-turn-${index}`,
    session_id: `bench-${index}`,
    run_id: `bench-run-${index}`,
    content: "oi",
  }) + "\n");
  const rss = [];
  while (!events.some((event) => event.type === "run.completed")) {
    const observed = rssBytes(child.pid);
    if (observed !== null) rss.push(observed);
    await new Promise((resolve) => setTimeout(resolve, 2));
    if (performance.now() - turnStartedAt > 3_000) throw new Error(`benchmark sample ${index} timed out`);
  }
  const turnMs = performance.now() - turnStartedAt;
  child.stdin.end();
  await new Promise((resolve) => child.once("close", resolve));
  return { initializedMs, turnMs, peakRssBytes: rss.length ? Math.max(...rss) : null };
}

const results = [];
for (let index = 0; index < Math.max(1, samples); index += 1) results.push(await measure(index));
const startup = results.map((result) => result.initializedMs);
const turn = results.map((result) => result.turnMs);
const rss = results.map((result) => result.peakRssBytes).filter((value) => value !== null);
console.log(JSON.stringify({
  samples: results,
  summary: {
    startupP50Ms: percentile(startup, 0.5),
    startupP95Ms: percentile(startup, 0.95),
    turnP50Ms: percentile(turn, 0.5),
    turnP95Ms: percentile(turn, 0.95),
    peakRssP95Bytes: rss.length ? percentile(rss, 0.95) : null,
    note: "fake provider, local core process, no UI/STT/provider-network cost",
  },
}, null, 2));
