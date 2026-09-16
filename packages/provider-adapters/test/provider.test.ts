import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { OpenAICompatibleTextProvider, FakeTextProvider, ProviderError, parseJsonToolCall, plannedTool, providerFromEnvironment } from "../src/index.js";

test("fake provider is textual, streams and can be cancelled", async () => {
  const provider = new FakeTextProvider();
  const controller = new AbortController();
  const chunks: string[] = [];
  for await (const chunk of provider.stream("oi", controller.signal)) chunks.push(chunk);
  assert.equal(chunks.join(""), "Recebi: oi");
  assert.equal(provider.capabilities.multimodal, false);
});

test("environment selects fake provider without exposing credentials", () => {
  const provider = providerFromEnvironment({ VOX_PROVIDER: "fake", VOX_PROVIDER_API_KEY: "not-used" });
  assert.equal(provider.id, "fake-text");
});

test("openai-compatible provider requires explicit endpoint and model", () => {
  assert.throws(() => providerFromEnvironment({ VOX_PROVIDER: "openai-compatible" }), (error) => error instanceof ProviderError && error.code === "CONFIG_ERROR");
});

test("openai-compatible provider rejects insecure remote endpoints", () => {
  assert.throws(() => providerFromEnvironment({
    VOX_PROVIDER: "openai-compatible",
    VOX_PROVIDER_BASE_URL: "http://example.com",
    VOX_PROVIDER_MODEL: "test-model",
  }), (error) => error instanceof ProviderError && error.code === "CONFIG_ERROR");
});

test("openai-compatible adapter consumes SSE without logging the credential", async () => {
  const server = createServer((request, response) => {
    assert.equal(request.headers.authorization, "Bearer secret-for-test");
    response.writeHead(200, { "content-type": "text/event-stream" });
    response.write('data: {"choices":[{"delta":{"content":"Olá"}}]}\n\n');
    response.write('data: {"choices":[{"delta":{"content":" Vox"}}]}\n\n');
    response.end("data: [DONE]\n\n");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  assert.ok(address && typeof address !== "string");
  const provider = new OpenAICompatibleTextProvider({
    baseUrl: `http://127.0.0.1:${address.port}`,
    model: "test-model",
    apiKey: "secret-for-test",
  });
  const chunks: string[] = [];
  for await (const chunk of provider.stream("oi", new AbortController().signal)) chunks.push(chunk);
  assert.equal(chunks.join(""), "Olá Vox");
  await new Promise<void>((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
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

test("planned tools classify native reads and writes before the broker", () => {
  const plan = plannedTool("crie um arquivo com este conteúdo");
  assert.equal(plan?.tool, "files.write");
  assert.equal(plan?.effect, "write");
  assert.equal(plannedTool("leia o clipboard")?.tool, "clipboard.read");
  assert.equal(plannedTool("copie isto para a área de transferência")?.tool, "clipboard.write");
  assert.equal(plannedTool("liste os arquivos")?.tool, "files.list");
  assert.equal(plannedTool("procure o arquivo relatório")?.tool, "files.search");
  assert.equal(plannedTool("abra o Discord")?.tool, "apps.launch");
  assert.equal(plannedTool("abra o Discord")?.effect, "external");
});
