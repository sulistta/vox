import test from "node:test";
import assert from "node:assert/strict";
import { compactConversation } from "../src/context.js";
import { RunLimitError } from "../src/limits.js";

test("the runtime conversation compactor preserves required turns and marks dropped history", () => {
  const system = "system";
  const result = compactConversation([
    { message: { role: "system", content: system }, required: true },
    { message: { role: "user", content: "objetivo antigo" }, required: false },
    { message: { role: "user", content: "ferramenta pendente" }, required: true },
    { message: { role: "user", content: "histórico descartável".repeat(30) }, required: false },
    { message: { role: "user", content: "pedido atual" }, required: true },
  ], 300);
  assert.equal(result.length >= 4, true);
  assert.equal(result[0]?.content, system);
  assert.equal(result.some((message) => message.content.includes("ferramenta pendente")), true);
  assert.equal(result.some((message) => message.content.includes("pedido atual")), true);
  assert.equal(result.some((message) => message.content.includes("mensagens antigas foram omitidas")), true);
  assert.equal(result.some((message) => message.content === "histórico descartável"), false);
});

test("the runtime conversation compactor keeps required context that fits and rejects tiny budgets", () => {
  const entries = [
    { message: { role: "system" as const, content: "system prompt" }, required: true },
    { message: { role: "user" as const, content: "observação obrigatória pendente" }, required: true },
  ];
  assert.deepEqual(compactConversation(entries, 512).map((message) => message.content), [
    "system prompt",
    "observação obrigatória pendente",
  ]);
  assert.throws(() => compactConversation(entries, 20), (error) => error instanceof RunLimitError);
});

