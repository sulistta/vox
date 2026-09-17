import readline from "node:readline";

const input = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
const send = (message) => process.stdout.write(`${JSON.stringify(message)}\n`);

input.on("line", (line) => {
  const message = JSON.parse(line);
  if (message.type === "initialize") {
    send({ type: "initialized", protocol: 1, runtime: process.version, build: "slow-turn-fixture", capabilities: [] });
  } else if (message.type === "session.open") {
    send({ type: "session.opened", request_id: message.request_id, session_id: message.session_id });
  } else if (message.type === "turn.start") {
    send({ type: "state.changed", session_id: message.session_id, run_id: message.run_id, state: "thinking" });
    // The core stays alive and continues answering heartbeats while a remote
    // provider has not produced its final response yet.
    setTimeout(() => {
      send({ type: "run.completed", session_id: message.session_id, run_id: message.run_id, seq: 1, content: "resposta lenta" });
    }, 320);
  } else if (message.type === "heartbeat") {
    send({ type: "heartbeat", request_id: message.request_id, runtime: process.version, healthy: true });
  }
});
