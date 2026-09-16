import test from "node:test";
import assert from "node:assert/strict";
import { RunBudget, RunLimitError } from "../src/limits.js";

test("run budget rejects oversized context and excessive steps", () => {
  const budget = new RunBudget({ maxPromptChars: 3, maxSteps: 1 });
  assert.throws(() => budget.validatePrompt("abcd"), (error) => error instanceof RunLimitError && error.code === "CONTEXT_LIMIT");
  budget.validatePrompt("ok");
  budget.consumeStep();
  assert.throws(() => budget.consumeStep(), (error) => error instanceof RunLimitError && error.code === "STEP_LIMIT");
});

test("run budget stops three identical observations", () => {
  const budget = new RunBudget({ stagnationLimit: 3 });
  budget.observe("same");
  budget.observe("same");
  assert.throws(() => budget.observe("same"), (error) => error instanceof RunLimitError && error.code === "STAGNATION_LIMIT");
});

test("run budget bounds accumulated provider output", () => {
  const budget = new RunBudget({ maxOutputChars: 4 });
  budget.validateOutput("ok");
  assert.throws(() => budget.validateOutput("too long"), (error) => error instanceof RunLimitError && error.code === "CONTEXT_LIMIT");
});
