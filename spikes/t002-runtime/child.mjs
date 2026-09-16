import readline from "node:readline";

const active = new Map();

function emit(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function sleep(ms, signal) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener("abort", () => {
      clearTimeout(timer);
      reject(new Error("cancelled"));
    }, { once: true });
  });
}

async function runTurn(message) {
  const controller = new AbortController();
  active.set(message.run_id, controller);
  try {
    emit({ type: "run.started", run_id: message.run_id, seq: 1 });
    const chunks = ["Vox ", "processou ", "o turno ", "com segurança."];
    let seq = 2;
    for (const delta of chunks) {
      await sleep(5, controller.signal);
      emit({ type: "message.delta", run_id: message.run_id, seq, delta });
      seq += 1;
    }
    emit({ type: "run.completed", run_id: message.run_id, seq, content: chunks.join("") });
  } catch (error) {
    emit({ type: "run.cancelled", run_id: message.run_id, seq: 2, reason: error.message });
  } finally {
    active.delete(message.run_id);
  }
}

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    emit({ type: "error", code: "INVALID_JSON" });
    return;
  }

  if (message.type === "initialize") {
    emit({ type: "initialized", protocol: 1, runtime: process.version, capabilities: ["stream", "cancel"] });
  } else if (message.type === "turn.start") {
    if (message.mode === "crash") {
      process.exit(17);
    }
    void runTurn(message);
  } else if (message.type === "turn.cancel") {
    active.get(message.run_id)?.abort();
  }
});
