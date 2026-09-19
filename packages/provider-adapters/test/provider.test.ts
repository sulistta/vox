import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import {
  OpenAICompatibleTextPlanner,
  ProviderError,
  TypeSafeJevAgent,
} from "../src/index.js";

async function fixture(handler: (body: string, response: import("node:http").ServerResponse) => void) {
  const server = createServer((request, response) => {
    let body = "";
    request.on("data", (chunk: Buffer) => { body += chunk; });
    request.on("end", () => handler(body, response));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") throw new Error("fixture address unavailable");
  return { baseUrl: `http://127.0.0.1:${address.port}`, close: () => new Promise<void>((resolve) => server.close(() => resolve())) };
}

const questions = [{ type: "choice" as const, id: "next", instructions: "Escolha", options: [{ id: "a", description: "A" }, { id: "b", description: "B" }] }];

test("Jev returns typed Choice answers with the resolved model", async () => {
  const api = await fixture((body, response) => {
    assert.match(body, /"questions"/u);
    response.setHeader("content-type", "application/json");
    response.end(JSON.stringify({ model: "jev-2026-09", answers: { next: { type: "choice", choice: "a", confidence: 0.91, probabilities: { a: 0.91, b: 0.09 } } } }));
  });
  try {
    const agent = new TypeSafeJevAgent({ baseUrl: api.baseUrl, apiKey: "test-key" });
    const result = await agent.decide({ state: "estado sem segredos", questions }, new AbortController().signal);
    assert.equal(result.model_ref, "jev-2026-09");
    assert.deepEqual(result.answers.next, { type: "choice", choice: "a", confidence: 0.91, probabilities: { a: 0.91, b: 0.09 } });
  } finally { await api.close(); }
});

test("Jev rejects answers outside the supplied catalog", async () => {
  const api = await fixture((_body, response) => response.end(JSON.stringify({ answers: { next: { type: "choice", choice: "outside", confidence: 1, probabilities: { a: 0.5, b: 0.5 } } } })));
  try {
    const agent = new TypeSafeJevAgent({ baseUrl: api.baseUrl, apiKey: "test-key" });
    await assert.rejects(agent.decide({ state: "state", questions }, new AbortController().signal), (error: unknown) => error instanceof ProviderError && error.code === "INVALID_RESPONSE");
  } finally { await api.close(); }
});

test("Jev preserves Score and Noul answers", async () => {
  const api = await fixture((_body, response) => response.end(JSON.stringify({ answers: {
    choice: { type: "choice", choice: "yes", confidence: 0.8, probabilities: { yes: 0.8, no: 0.2 } },
    score: { type: "score", score: 0.9, confidence: 0.9, probabilities: { "0": 0.1, "1": 0.9 } },
    verified: { type: "noul", noul: 0.73 },
  } })));
  try {
    const agent = new TypeSafeJevAgent({ baseUrl: api.baseUrl, apiKey: "test-key" });
    const result = await agent.decide({ state: "state", questions: [
      { type: "choice", id: "choice", instructions: "Escolha", options: [{ id: "yes", description: "sim" }, { id: "no", description: "não" }] },
      { type: "score", id: "score", instructions: "Pontue", levels: [{ id: "low", description: "baixa" }, { id: "high", description: "alta" }] },
      { type: "noul", id: "verified", statement: "O resultado foi verificado." },
    ] }, new AbortController().signal);
    assert.equal(result.answers.score?.type, "score");
    assert.deepEqual(result.answers.verified, { type: "noul", noul: 0.73 });
  } finally { await api.close(); }
});

test("Jev classifies authentication and rate errors", async () => {
  const auth = await fixture((_body, response) => { response.statusCode = 401; response.end("no"); });
  const rate = await fixture((_body, response) => { response.statusCode = 429; response.end("slow"); });
  try {
    await assert.rejects(new TypeSafeJevAgent({ baseUrl: auth.baseUrl, apiKey: "test" }).decide({ state: "state", questions }, new AbortController().signal), (error: unknown) => error instanceof ProviderError && error.code === "AUTH_ERROR");
    await assert.rejects(new TypeSafeJevAgent({ baseUrl: rate.baseUrl, apiKey: "test" }).decide({ state: "state", questions }, new AbortController().signal), (error: unknown) => error instanceof ProviderError && error.code === "RATE_LIMIT" && error.retryable);
  } finally { await auth.close(); await rate.close(); }
});

test("Jev respects timeout and caller cancellation", async () => {
  const api = await fixture((_body, response) => setTimeout(() => response.end("{}"), 100));
  try {
    await assert.rejects(new TypeSafeJevAgent({ baseUrl: api.baseUrl, apiKey: "test", timeoutMs: 10 }).decide({ state: "state", questions }, new AbortController().signal), (error: unknown) => error instanceof ProviderError && error.code === "TIMEOUT");
    const controller = new AbortController(); controller.abort();
    await assert.rejects(new TypeSafeJevAgent({ baseUrl: api.baseUrl, apiKey: "test" }).decide({ state: "state", questions }, controller.signal), (error: unknown) => error instanceof ProviderError && error.code === "CANCELLED");
  } finally { await api.close(); }
});

test("text planner returns data and never exposes a tool dispatch operation", async () => {
  const api = await fixture((_body, response) => response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify({ objective: "listar", app_hints: [], targets: [], expected_result: "lista" }) } }] })));
  try {
    const planner = new OpenAICompatibleTextPlanner({ baseUrl: api.baseUrl, apiKey: "test", model: "text" });
    assert.equal(typeof (planner as { execute?: unknown }).execute, "undefined");
    assert.equal((await planner.interpretTask("liste", new AbortController().signal)).objective, "listar");
  } finally { await api.close(); }
});

test("text connection probe reads only the bounded model catalog", async () => {
  const api = await fixture((_body, response) => {
    response.end(JSON.stringify({ data: [{ id: "text-a" }, { id: "text-b" }] }));
  });
  try {
    const planner = new OpenAICompatibleTextPlanner({ baseUrl: api.baseUrl, apiKey: "test", model: "text" });
    assert.deepEqual(await planner.probe(new AbortController().signal), {
      available: true,
      model_ids: ["text-a", "text-b"],
    });
  } finally { await api.close(); }
});

test("candidate repair is bounded and reports the invalid field without echoing content", async () => {
  for (const repairSucceeds of [true, false]) {
    let calls = 0;
    const api = await fixture((body, response) => {
      calls += 1;
      if (calls === 2) {
        assert.match(body, /verification deve ser texto/u);
        assert.doesNotMatch(body, /private-marker/u);
      }
      response.end(JSON.stringify({ choices: [{ message: { content: JSON.stringify({
        candidates: [{ id: "inspect", tool: "desktop.list_windows", arguments: {},
          label: "Listar", verification: repairSucceeds && calls === 2 ? "Lista observada" : { detail: "private-marker" } }],
      }) } }] }));
    });
    try {
      const planner = new OpenAICompatibleTextPlanner({ baseUrl: api.baseUrl, apiKey: "test", model: "text" });
      const result = planner.proposeCandidates({
        task: { objective: "listar", app_hints: [], targets: [], expected_result: "lista" },
        state: "estado", tools: [],
      }, new AbortController().signal);
      if (repairSucceeds) assert.equal((await result)[0]?.tool, "desktop.list_windows");
      else await assert.rejects(result, /verification deve ser texto/u);
      assert.equal(calls, 2);
    } finally { await api.close(); }
  }
});
