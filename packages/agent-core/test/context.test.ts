import test from "node:test";
import assert from "node:assert/strict";
import { compactContext, ContextCompactionError, formatContext } from "../src/context.js";

test("context compaction preserves objective and uncertain effects before recent history", () => {
  const result = compactContext([
    { role: "system", content: "restrição: nunca apagar dados" },
    { role: "user", content: "objetivo antigo" },
    { role: "assistant", content: "observação longa que pode ser removida porque não é um fato obrigatório ".repeat(5) },
    { role: "tool", content: "efeito ainda incerto", effect: "unknown" },
    { role: "assistant", content: "última observação" },
  ], 300);
  assert.equal(result.dropped > 0, true);
  assert.equal(result.messages.some((message) => message.content.includes("nunca apagar")), true);
  assert.equal(result.messages.some((message) => message.effect === "unknown"), true);
  assert.equal(result.messages.some((message) => message.content.includes("última observação")), true);
  assert.equal(result.messages.some((message) => message.content.includes("contexto compactado")), true);
});

test("context compaction refuses to drop required facts when the budget is too small", () => {
  assert.throws(
    () => compactContext([{ role: "system", content: "x".repeat(300) }, { role: "tool", content: "pending", effect: "pending" }], 256),
    (error) => error instanceof ContextCompactionError,
  );
});

test("formatted context remains plain text and appends the current user turn", () => {
  assert.equal(formatContext([{ role: "assistant", content: "linha" }], "agora"), "[assistant] linha\n[user] agora");
});
