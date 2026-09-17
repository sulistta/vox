import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import {
  OpenAICompatibleTextProvider,
  FakeTextProvider,
  ProviderError,
  displayEffectForTool,
  parseJsonToolCall,
  parseModelDecision,
  probeOpenAICompatible,
  providerFromEnvironment,
  redactJsonForModel,
  redactTextForModel,
} from "../src/index.js";

async function startServer(handler: (request: import("node:http").IncomingMessage) => { status: number; body: string }): Promise<{ url: string; close: () => Promise<void> }> {
  const server = createServer((request, response) => {
    const result = handler(request);
    response.writeHead(result.status, { "content-type": "application/json" });
    response.end(result.body);
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () => new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve())),
  };
}

async function withMockedFetch<T>(response: Response, operation: () => Promise<T>): Promise<T> {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async () => response) as typeof fetch;
  try {
    return await operation();
  } finally {
    globalThis.fetch = originalFetch;
  }
}

function streamingResponse(getReader: () => ReadableStreamDefaultReader<Uint8Array>): Response {
  return {
    ok: true,
    body: { getReader } as unknown as ReadableStream<Uint8Array>,
    headers: new Headers({ "content-type": "text/event-stream" }),
  } as Response;
}

test("default fake provider is textual and cannot propose desktop tools", async () => {
  const provider = new FakeTextProvider();
  const controller = new AbortController();
  const chunks: string[] = [];
  for await (const chunk of provider.stream("oi", controller.signal)) chunks.push(chunk);
  const decision = parseModelDecision(chunks.join(""));
  assert.equal(decision?.type, "final");
  assert.match(decision?.type === "final" ? decision.content : "", /demonstração/u);
  assert.equal(provider.capabilities.multimodal, false);
  assert.equal(provider.capabilities.context_chars, 16 * 1024);
});

test("environment selects fake provider without exposing credentials", () => {
  const provider = providerFromEnvironment({ VOX_PROVIDER: "fake", VOX_PROVIDER_API_KEY: "not-used" });
  assert.equal(provider.id, "fake-text");
  assert.equal(providerFromEnvironment({ VOX_PROVIDER: "fake-tools" }).id, "fake-tools-fixture");
});

test("openai-compatible provider requires explicit endpoint and model", () => {
  assert.throws(() => providerFromEnvironment({ VOX_PROVIDER: "openai-compatible" }), (error) => error instanceof ProviderError && error.code === "CONFIG_ERROR");
});

test("provider probe reports bounded model ids without exposing response bodies", async () => {
  const server = await startServer((request) => {
    if (request.url === "/models") return { status: 200, body: JSON.stringify({ data: [{ id: "local-a" }, { id: "local-b" }, { nope: true }] }) };
    return { status: 404, body: "missing" };
  });
  try {
    const result = await probeOpenAICompatible({ baseUrl: server.url, model: "local-a" });
    assert.deepEqual(result, { available: true, model_ids: ["local-a", "local-b"] });
  } finally {
    await server.close();
  }
});

test("openai-compatible provider rejects insecure remote endpoints", () => {
  assert.throws(() => providerFromEnvironment({
    VOX_PROVIDER: "openai-compatible",
    VOX_PROVIDER_BASE_URL: "http://example.com",
    VOX_PROVIDER_MODEL: "test-model",
  }), (error) => error instanceof ProviderError && error.code === "CONFIG_ERROR");
});

test("openai-compatible adapter consumes SSE without logging the credential", async () => {
  const standaloneProviderKey = "sk-or-v1-0123456789abcdef0123456789abcdef";
  const portugueseSecretText = "senha=senha-local segredo:segredo-local chave=chave-local credenciais:credencial-local";
  let requestBody = "";
  const server = createServer((request, response) => {
    assert.equal(request.headers.authorization, "Bearer secret-for-test");
    request.setEncoding("utf8");
    request.on("data", (chunk: string) => { requestBody += chunk; });
    request.on("end", () => {
      response.writeHead(200, { "content-type": "text/event-stream" });
      response.write('data: {"choices":[{"delta":{"content":"Olá"}}]}\n\n');
      response.write('data: {"choices":[{"delta":{"content":" Vox"}}]}\n\n');
      response.end("data: [DONE]\n\n");
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    const address = server.address();
    assert.ok(address && typeof address !== "string");
    const provider = new OpenAICompatibleTextProvider({
      baseUrl: `http://127.0.0.1:${address.port}`,
      model: "test-model",
      apiKey: "secret-for-test",
    });
    const chunks: string[] = [];
  for await (const chunk of provider.stream(`${standaloneProviderKey} ${portugueseSecretText} senha=\"senha com espaço\"`, new AbortController().signal)) chunks.push(chunk);
    assert.equal(chunks.join(""), "Olá Vox");
    assert.equal(requestBody.includes(standaloneProviderKey), false);
    assert.equal(requestBody.includes("senha-local"), false);
    assert.equal(requestBody.includes("segredo-local"), false);
    assert.equal(requestBody.includes("chave-local"), false);
  assert.equal(requestBody.includes("credencial-local"), false);
  assert.equal(requestBody.includes("senha com espaço"), false);
    assert.equal(requestBody.includes("[REDACTED]"), true);
  } finally {
    await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
  }
});

test("openai-compatible adapter accepts JSON responses and trailing SSE data", async () => {
  const jsonServer = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.end('{"choices":[{"message":{"content":"Resposta JSON"}}]}');
  });
  await new Promise<void>((resolve) => jsonServer.listen(0, "127.0.0.1", resolve));
  const jsonAddress = jsonServer.address();
  assert.ok(jsonAddress && typeof jsonAddress !== "string");
  const jsonProvider = new OpenAICompatibleTextProvider({
    baseUrl: "http://127.0.0.1:" + jsonAddress.port,
    model: "test-model",
  });
  const jsonChunks: string[] = [];
  for await (const chunk of jsonProvider.stream("oi", new AbortController().signal)) jsonChunks.push(chunk);
  assert.deepEqual(jsonChunks, ["Resposta JSON"]);
  await new Promise<void>((resolve, reject) => jsonServer.close((error) => error ? reject(error) : resolve()));

  const sseServer = createServer((_request, response) => {
    response.writeHead(200, { "content-type": "text/event-stream" });
    response.end('data: {"choices":[{"delta":{"content":"fim"}}]}');
  });
  await new Promise<void>((resolve) => sseServer.listen(0, "127.0.0.1", resolve));
  const sseAddress = sseServer.address();
  assert.ok(sseAddress && typeof sseAddress !== "string");
  const sseProvider = new OpenAICompatibleTextProvider({
    baseUrl: "http://127.0.0.1:" + sseAddress.port,
    model: "test-model",
  });
  const sseChunks: string[] = [];
  for await (const chunk of sseProvider.stream("oi", new AbortController().signal)) sseChunks.push(chunk);
  assert.deepEqual(sseChunks, ["fim"]);
  await new Promise<void>((resolve, reject) => sseServer.close((error) => error ? reject(error) : resolve()));
});

test("openai-compatible adapter bounds unterminated SSE events", async () => {
  const payload = new TextEncoder().encode(`data: ${"x".repeat(256 * 1024)}`);
  let reads = 0;
  const reader = {
    read: async () => {
      reads += 1;
      return reads === 1 ? { done: false as const, value: payload } : { done: true as const, value: undefined };
    },
  } as unknown as ReadableStreamDefaultReader<Uint8Array>;
  const provider = new OpenAICompatibleTextProvider({ baseUrl: "http://127.0.0.1:1", model: "test-model" });

  await withMockedFetch(streamingResponse(() => reader), async () => {
    await assert.rejects(async () => {
      for await (const _chunk of provider.stream("oi", new AbortController().signal)) {
        // The malformed event must fail before producing output.
      }
    }, (error) => error instanceof ProviderError
      && error.code === "INVALID_STREAM"
      && error.retryable === false);
  });
});

test("openai-compatible adapter turns SSE reader failures into retryable network errors", async () => {
  const provider = new OpenAICompatibleTextProvider({ baseUrl: "http://127.0.0.1:1", model: "test-model" });
  const failure = new Error("reader transport api_key=reader-secret");
  const reader = {
    read: async () => { throw failure; },
  } as unknown as ReadableStreamDefaultReader<Uint8Array>;

  await withMockedFetch(streamingResponse(() => reader), async () => {
    await assert.rejects(async () => {
      for await (const _chunk of provider.stream("oi", new AbortController().signal)) {
        // The read is expected to fail before producing a chunk.
      }
    }, (error) => error instanceof ProviderError
      && error.code === "NETWORK_ERROR"
      && error.retryable === true
      && !error.message.includes("reader-secret")
      && error.message.includes("[REDACTED]"));
  });
});

test("openai-compatible adapter turns SSE reader acquisition failures into retryable network errors", async () => {
  const provider = new OpenAICompatibleTextProvider({ baseUrl: "http://127.0.0.1:1", model: "test-model" });
  const failure = new Error("stream setup token=reader-setup-secret");

  await withMockedFetch(streamingResponse(() => { throw failure; }), async () => {
    await assert.rejects(async () => {
      for await (const _chunk of provider.stream("oi", new AbortController().signal)) {
        // The reader is expected to fail before producing a chunk.
      }
    }, (error) => error instanceof ProviderError
      && error.code === "NETWORK_ERROR"
      && error.retryable === true
      && !error.message.includes("reader-setup-secret")
      && error.message.includes("[REDACTED]"));
  });
});

test("provider error details redact credentials returned by the server", async () => {
  const server = createServer((_request, response) => {
    response.writeHead(401, { "content-type": "text/plain" });
    response.end('{"error":{"api_key":"super-secret-token"}}');
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const provider = new OpenAICompatibleTextProvider({
    baseUrl: "http://127.0.0.1:" + address.port,
    model: "test-model",
  });
  await assert.rejects(async () => {
    for await (const _chunk of provider.stream("oi", new AbortController().signal)) {
      // The request is expected to fail before producing a chunk.
    }
  }, (error) => error instanceof ProviderError
    && error.code === "AUTH_ERROR"
    && !error.message.includes("super-secret-token")
    && error.message.includes("[REDACTED]"));
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
});

test("JSON tool fallback accepts only a bounded object-shaped call", () => {
  assert.deepEqual(parseJsonToolCall('{"tool":"files.read","arguments":{"path":"a.txt"}}'), {
    tool: "files.read",
    arguments: { path: "a.txt" },
  });
  assert.equal(parseJsonToolCall('{"tool":"rm -rf /","arguments":{}}'), undefined);
  assert.equal(parseJsonToolCall('{"tool":"files.read","arguments":[] }'), undefined);
});

test("model decisions are structured and tool effect labels never authorize calls", () => {
  assert.deepEqual(parseModelDecision('{"type":"tool_call","tool":"files.read","arguments":{"path":"a.txt"}}'), {
    type: "tool_calls",
    calls: [{ tool: "files.read", arguments: { path: "a.txt" } }],
  });
  assert.deepEqual(parseModelDecision('{"type":"final","content":"feito"}'), {
    type: "final",
    content: "feito",
  });
  assert.deepEqual(parseModelDecision('```json\n{"type":"final","content":"feito"}\n```'), {
    type: "final",
    content: "feito",
  });
  assert.equal(parseModelDecision('{"type":"tool_call","tool":"files.read","arguments":{},"extra":true}'), undefined);
  assert.equal(parseModelDecision("vou executar files.read"), undefined);
  assert.equal(parseModelDecision('vou executar\n```json\n{"type":"final","content":"feito"}\n```'), undefined);
  assert.equal(displayEffectForTool("files.read"), "read");
  assert.equal(displayEffectForTool("files.write"), "write");
  assert.equal(displayEffectForTool("unknown.tool"), "arbitrary");
});

test("redaction protects text and structured tool observations before model reuse", () => {
  const standaloneProviderKey = "sk-or-v1-0123456789abcdef0123456789abcdef";
  const text = redactTextForModel(`Bearer top-secret API_KEY=another-secret password="do-not-send with spaces" client_secret: third-secret senha=senha-local segredo:segredo-local chave=chave-local credenciais:credencial-local ${standaloneProviderKey}`);
  assert.equal(text.includes("top-secret"), false);
  assert.equal(text.includes("another-secret"), false);
  assert.equal(text.includes("do-not-send"), false);
  assert.equal(text.includes("with spaces"), false);
  assert.equal(text.includes("third-secret"), false);
  assert.equal(text.includes("senha-local"), false);
  assert.equal(text.includes("segredo-local"), false);
  assert.equal(text.includes("chave-local"), false);
  assert.equal(text.includes("credencial-local"), false);
  assert.equal(text.includes(standaloneProviderKey), false);
  assert.equal(text.includes("[REDACTED]"), true);
  assert.deepEqual(redactJsonForModel({
    authorization: "Bearer hidden",
    nested: {
      token: "nested-secret",
      password: "nested-password",
      senha: "nested-password-portuguese",
      segredo: "nested-secret-portuguese",
      chave: "nested-key-portuguese",
      credenciais: "nested-credential-portuguese",
      safe: "ok",
    },
  }), {
    authorization: "[REDACTED]",
    nested: {
      token: "[REDACTED]",
      password: "[REDACTED]",
      senha: "[REDACTED]",
      segredo: "[REDACTED]",
      chave: "[REDACTED]",
      credenciais: "[REDACTED]",
      safe: "ok",
    },
  });
});
