import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";
import { MAX_MESSAGE_BYTES, ProtocolError, encodeMessage, parseMessage } from "../src/index.js";
import { ipcSchema } from "../src/ipc-schema.js";

const root = fileURLToPath(new URL("../../../", import.meta.url));
const cases = JSON.parse(readFileSync(resolve(root, "tests/contracts/boundary-cases.json"), "utf8")) as {
  name: string; expect: "valid" | "invalid"; message?: unknown; line?: string;
}[];
function expand(value: any): any {
  if (value && typeof value === "object" && "$repeat" in value) return value.$repeat.repeat(value.count);
  if (Array.isArray(value)) return value.map(expand);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, expand(item)]));
  return value;
}
for (const fixture of cases) {
  test(`shared boundary: ${fixture.name}`, () => {
    const expanded = expand(fixture.message) as Record<string, unknown> | undefined;
    const message = expanded && ["turn.start", "tool.started", "tool.execute.requested"].includes(String(expanded.type))
      ? { ...expanded, task_grant_id: "fixture-grant" }
      : expanded;
    const line = fixture.line ?? JSON.stringify(message);
    if (fixture.expect === "valid") assert.doesNotThrow(() => parseMessage(line));
    else assert.throws(() => parseMessage(line), ProtocolError);
  });
}

test("bundled schema exactly matches canonical schema and uses supported keywords", () => {
  assert.deepEqual(ipcSchema, JSON.parse(readFileSync(resolve(root, "schemas/ipc.schema.json"), "utf8")));
  const supported = new Set(["type", "const", "enum", "minLength", "maxLength", "x-maxUtf8Bytes", "minimum", "maximum", "required", "properties", "items", "minItems", "maxItems", "additionalProperties"]);
  function check(rule: Record<string, unknown>): void {
    for (const key of Object.keys(rule)) assert.ok(supported.has(key), `unsupported keyword ${key}`);
    if ("additionalProperties" in rule) assert.equal(rule.additionalProperties, true);
    for (const child of Object.values(rule.properties ?? {})) check(child as Record<string, unknown>);
    if (rule.items) check(rule.items as Record<string, unknown>);
  }
  for (const rule of Object.values(ipcSchema.$defs)) check(rule);
});

const contract = (name: string, overrides: Record<string, unknown> = {}): string => {
  const value = JSON.parse(
    readFileSync(
      resolve(fileURLToPath(new URL("../../../", import.meta.url)), "tests/contracts", name),
      "utf8",
    ),
  ) as Record<string, unknown>;
  return JSON.stringify({ ...value, task_grant_id: "contract-grant", ...overrides });
};

test("accepts a valid turn and preserves structured fields", () => {
  const encoded = encodeMessage({
    type: "turn.start",
    request_id: "req-1",
    session_id: "session-1",
    run_id: "run-1",
    task_grant_id: "grant-1",
    content: "mostrar janelas",
  });
  assert.equal(encoded.endsWith("\n"), true);
  assert.deepEqual(parseMessage(encoded.trim()), {
    type: "turn.start",
    request_id: "req-1",
    session_id: "session-1",
    run_id: "run-1",
    task_grant_id: "grant-1",
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

test("accepts bounded context and rejects an invalid effect", () => {
  const parsed = parseMessage(contract("valid-turn.json", {
    context: [
      { role: "system", content: "keep this constraint" },
      { role: "tool", content: "uncertain", effect: "unknown" },
    ],
  }));
  assert.equal(parsed.type, "turn.start");
  assert.throws(
    () => parseMessage(contract("valid-turn.json", { context: [{ role: "tool", content: "x", effect: "success" }] })),
    (error: unknown) => error instanceof ProtocolError && error.code === "INVALID_FIELD",
  );
});

test("accepts only bounded, explicitly redacted model activity", () => {
  const valid = {
    type: "model.requested",
    session_id: "session-activity",
    run_id: "run-activity",
    round: 1,
    provider: "openai-compatible",
    model_ref: "local-model",
    redacted: true,
    messages: [
      { role: "system", content: "Use JSON decisions." },
      { role: "user", content: "API_KEY=[REDACTED]" },
    ],
  };
  assert.equal(parseMessage(JSON.stringify(valid)).type, "model.requested");
  assert.throws(
    () => parseMessage(JSON.stringify({ ...valid, redacted: false })),
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
