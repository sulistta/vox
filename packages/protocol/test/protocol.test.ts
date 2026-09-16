import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { MAX_MESSAGE_BYTES, ProtocolError, encodeMessage, parseMessage } from "../src/index.js";

const contract = (name: string, overrides: Record<string, unknown> = {}): string => {
  const value = JSON.parse(
    readFileSync(
      resolve(fileURLToPath(new URL("../../../", import.meta.url)), "tests/contracts", name),
      "utf8",
    ),
  ) as Record<string, unknown>;
  return JSON.stringify({ ...value, ...overrides });
};

test("accepts a valid turn and preserves structured fields", () => {
  const encoded = encodeMessage({
    type: "turn.start",
    request_id: "req-1",
    session_id: "session-1",
    run_id: "run-1",
    content: "mostrar janelas",
  });
  assert.equal(encoded.endsWith("\n"), true);
  assert.deepEqual(parseMessage(encoded.trim()), {
    type: "turn.start",
    request_id: "req-1",
    session_id: "session-1",
    run_id: "run-1",
    content: "mostrar janelas",
  });
});

test("accepts a reviewed voice turn and rejects an unknown source", () => {
  const voice = parseMessage(contract("valid-turn.json", { source: "voice" }));
  assert.equal(voice.type, "turn.start");
  assert.equal(voice.source, "voice");
  assert.throws(
    () => parseMessage(contract("valid-turn.json", { source: "microphone" })),
    (error: unknown) => error instanceof ProtocolError && error.code === "INVALID_FIELD",
  );
});

test("rejects invalid JSON and unknown types", () => {
  assert.throws(() => parseMessage("{"), (error) => error instanceof ProtocolError && error.code === "INVALID_JSON");
  assert.throws(() => parseMessage('{"type":"delete_everything"}'), (error) => error instanceof ProtocolError && error.code === "UNKNOWN_TYPE");
});

test("accepts a bounded heartbeat request and rejects an empty id", () => {
  assert.deepEqual(parseMessage('{"type":"heartbeat","request_id":"hb-1"}'), {
    type: "heartbeat",
    request_id: "hb-1",
  });
  assert.throws(
    () => parseMessage('{"type":"heartbeat","request_id":""}'),
    (error) => error instanceof ProtocolError && error.code === "INVALID_FIELD",
  );
});

test("rejects oversized frames and incompatible versions", () => {
  assert.throws(() => parseMessage(JSON.stringify({ type: "message.delta", delta: "x".repeat(MAX_MESSAGE_BYTES) })), (error) => error instanceof ProtocolError && error.code === "MESSAGE_TOO_LARGE");
  assert.throws(() => parseMessage(JSON.stringify({ type: "initialized", protocol: 99 })), (error) => error instanceof ProtocolError && error.code === "VERSION_MISMATCH");
});

test("accepts the shared cross-language contract fixtures", () => {
  assert.equal(parseMessage(contract("valid-turn.json")).type, "turn.start");
  assert.equal(parseMessage(contract("valid-tool-result.json")).type, "tool.result");
});

test("rejects the shared incompatible-version fixture", () => {
  assert.throws(
    () => parseMessage(contract("invalid-version.json")),
    (error) => error instanceof ProtocolError && error.code === "VERSION_MISMATCH",
  );
});
