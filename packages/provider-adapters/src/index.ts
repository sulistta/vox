import type { EffectClass, JsonObject, JsonValue } from "@vox/protocol";

export interface ProviderCapabilities {
  streaming: boolean;
  native_tools: boolean;
  json_mode: boolean;
  multimodal: false;
  context_chars: number;
  max_output_chars: number;
}

/**
 * The adapters deliberately use ordinary chat messages. Native provider tool
 * calling is not part of the v1 contract: the model returns a JSON decision
 * in text and the broker remains the only authority that can execute it.
 */
export type ProviderRole = "system" | "user" | "assistant";

export interface ProviderMessage {
  role: ProviderRole;
  content: string;
}

export type ProviderInput = string | readonly ProviderMessage[];

export interface TextProvider {
  readonly id: string;
  readonly model_ref?: string;
  readonly capabilities: ProviderCapabilities;
  stream(input: ProviderInput, signal: AbortSignal): AsyncIterable<string>;
}

export interface ProviderProbeResult {
  available: boolean;
  model_ids: string[];
  status?: number;
  error?: string;
}

export interface NormalizedToolCall {
  tool: string;
  arguments: JsonObject;
}

export type ModelDecision =
  | { type: "tool_calls"; calls: NormalizedToolCall[] }
  | { type: "final"; content: string };

export interface TextToolDescription {
  name: string;
  effect: EffectClass;
  arguments: string;
}

/**
 * This catalog gives text-only models enough context to propose a call. It is
 * descriptive only: the Rust broker independently validates every tool,
 * argument, scope and approval before an effect can happen.
 */
export const TEXT_TOOL_CATALOG: readonly TextToolDescription[] = [
  { name: "desktop.list_windows", effect: "read", arguments: "{}" },
  { name: "desktop.snapshot", effect: "read", arguments: "{app_pid?}" },
  { name: "desktop.query", effect: "read", arguments: "{snapshot_id, query}" },
  { name: "desktop.wait_for", effect: "read", arguments: "{query, timeout_ms?, app_pid?}" },
  { name: "desktop.act", effect: "arbitrary", arguments: "{snapshot_id, element_ref, action, value?}" },
  { name: "clipboard.read", effect: "read", arguments: "{}" },
  { name: "clipboard.write", effect: "write", arguments: "{text}" },
  { name: "files.read", effect: "read", arguments: "{path}" },
  { name: "files.search", effect: "read", arguments: "{root?, query, limit?}" },
  { name: "files.list", effect: "read", arguments: "{root?, limit?}" },
  { name: "files.write", effect: "write", arguments: "{path, content, overwrite?}" },
  { name: "files.move", effect: "write", arguments: "{source, destination, overwrite?}" },
  { name: "process.list", effect: "read", arguments: "{limit?}" },
  { name: "process.terminate", effect: "external", arguments: "{pid, command?, start_time?}" },
  { name: "apps.resolve", effect: "read", arguments: "{name}" },
  { name: "apps.launch", effect: "external", arguments: "{program, argv?}" },
  { name: "paths.open", effect: "external", arguments: "{path}" },
  { name: "shell.exec", effect: "arbitrary", arguments: "{program, argv?, cwd?, timeout_ms?}" },
] as const;

const toolEffects = new Map(TEXT_TOOL_CATALOG.map(({ name, effect }) => [name, effect]));
const MAX_TOOL_CALLS_PER_DECISION = 8;
const MAX_TOOL_ARGUMENT_BYTES = 64 * 1024;
const MAX_FINAL_CONTENT_CHARS = 256 * 1024;
// A streaming provider can keep sending a line without a newline. Keep the
// retained fragment bounded so a malformed or hostile SSE response cannot
// grow the core process without limit.
const MAX_UNTERMINATED_SSE_BUFFER_CHARS = 256 * 1024;

export class ProviderError extends Error {
  constructor(readonly code: string, message: string, readonly retryable: boolean) {
    super(message);
    this.name = "ProviderError";
  }
}

function transportError(error: unknown, signal: AbortSignal, fallback: string): ProviderError {
  if (signal.aborted) return new ProviderError("CANCELLED", "provider cancelled", false);
  return new ProviderError(
    "NETWORK_ERROR",
    redactTextForModel(error instanceof Error ? error.message : fallback),
    true,
  );
}

export function redactTextForModel(value: string): string {
  return value
    .replace(/Bearer\s+[^\s,}]+/giu, "Bearer [REDACTED]")
    // People often paste an OpenAI-compatible key by itself instead of using
    // a `token=` label. Preserve neither the prefix nor its value before a
    // chat message, error or tool observation crosses the provider boundary.
    .replace(/\bsk-[a-z0-9_-]{16,}\b/giu, "[REDACTED]")
    // Keep this vocabulary aligned with the native UI and SQLite redactors:
    // tool observations and direct adapter calls can contain Portuguese keys.
    .replace(
      /(["']?(?:access[_-]?token|api[_-]?key|authorization|client[_-]?secret|cookie|credential(?:s)?|credencial|credenciais|pass(?:word|phrase)?|private[_-]?key|refresh[_-]?token|secret|token|senha(?:s)?|segredo(?:s)?|chave(?:s)?)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,}]+)/giu,
      "$1[REDACTED]",
    );
}

const sensitiveKey = /(?:access[_-]?token|api[_-]?key|authorization|client[_-]?secret|cookie|credential(?:s)?|credencial|credenciais|pass(?:word|phrase)?|private[_-]?key|refresh[_-]?token|secret|token|senha(?:s)?|segredo(?:s)?|chave(?:s)?)/iu;

/** Redact common secret-bearing values before a tool observation is reused as model input. */
export function redactJsonForModel(value: JsonValue): JsonValue {
  if (typeof value === "string") return redactTextForModel(value);
  if (value === null || typeof value === "number" || typeof value === "boolean") return value;
  if (Array.isArray(value)) return value.map((item) => redactJsonForModel(item));
  const redacted: JsonObject = {};
  for (const [key, item] of Object.entries(value)) {
    redacted[key] = sensitiveKey.test(key) ? "[REDACTED]" : redactJsonForModel(item);
  }
  return redacted;
}

/** This label is for activity UI only; it never authorizes a model-proposed call. */
export function displayEffectForTool(tool: string): EffectClass {
  return toolEffects.get(tool) ?? "arbitrary";
}

function normalizeInput(input: ProviderInput): ProviderMessage[] {
  const messages = typeof input === "string" ? [{ role: "user" as const, content: input }] : [...input];
  if (messages.length === 0 || messages.length > 128) {
    throw new ProviderError("INVALID_REQUEST", "provider request must contain between 1 and 128 messages", false);
  }
  return messages.map((message) => {
    if (!message || !["system", "user", "assistant"].includes(message.role) || typeof message.content !== "string") {
      throw new ProviderError("INVALID_REQUEST", "provider request contains an invalid message", false);
    }
    if (message.content.length > 1024 * 1024) {
      throw new ProviderError("INVALID_REQUEST", "provider message exceeds the size limit", false);
    }
    // Every provider receives normalized messages through this function.
    // Redact at that final shared boundary so direct adapter users cannot
    // bypass the protection that agent-core applies to normal conversations.
    return { role: message.role, content: redactTextForModel(message.content) };
  });
}

function validateEndpoint(baseUrl: string): void {
  let parsed: URL;
  try {
    parsed = new URL(baseUrl);
  } catch {
    throw new ProviderError("CONFIG_ERROR", "provider endpoint is not a valid URL", false);
  }
  const localHost = ["localhost", "127.0.0.1", "::1"].includes(parsed.hostname);
  if (parsed.protocol !== "https:" && !(parsed.protocol === "http:" && localHost)) {
    throw new ProviderError("CONFIG_ERROR", "provider endpoint must use HTTPS; HTTP is allowed only for localhost", false);
  }
  if (parsed.username || parsed.password) {
    throw new ProviderError("CONFIG_ERROR", "provider credentials must not be embedded in the endpoint URL", false);
  }
}

export class FakeTextProvider implements TextProvider {
  readonly id: string;
  readonly model_ref = "fake-text";
  readonly capabilities = {
    streaming: true,
    native_tools: false,
    json_mode: false,
    multimodal: false,
    context_chars: 16 * 1024,
    max_output_chars: 64 * 1024,
  } as const;

  /**
   * The ordinary fake provider is a harmless setup/demo mode. Tool proposals
   * exist only behind this explicit test fixture flag so a default desktop
   * launch cannot turn keyword matching into a real desktop effect.
   */
  constructor(private readonly toolFixture = false) {
    this.id = toolFixture ? "fake-tools-fixture" : "fake-text";
  }

  async *stream(input: ProviderInput, signal: AbortSignal): AsyncIterable<string> {
    const messages = normalizeInput(input);
    const latestUser = [...messages].reverse().find((message) => message.role === "user")?.content ?? "";
    const response = this.toolFixture && latestUser.startsWith("TOOL_RESULT\n")
      ? JSON.stringify({ type: "final", content: fakeFinalAfterTool(latestUser) })
      : this.toolFixture
        ? (() => {
          const call = fakeToolCallFor(latestUser);
          return call
            ? JSON.stringify({ type: "tool_calls", calls: [call] })
            : JSON.stringify({ type: "final", content: `Recebi: ${latestUser}` });
          })()
        : JSON.stringify({
          type: "final",
          content: "O modo de demonstração está ativo. Configure um endpoint OpenAI-compatible e um modelo em Preferências antes de pedir ações no computador.",
        });
    for (const chunk of response.match(/.{1,18}/gu) ?? [response]) {
      if (signal.aborted) throw new ProviderError("CANCELLED", "provider cancelled", false);
      await new Promise((resolve) => setTimeout(resolve, 3));
      yield chunk;
    }
  }
}

/**
 * The fake provider is a deterministic model fixture. It chooses a small set
 * of calls itself so the IPC and broker integration can be exercised without
 * a network model. Agent-core never calls this helper or matches user text.
 */
function fakeToolCallFor(prompt: string): NormalizedToolCall | undefined {
  const normalized = prompt.toLowerCase();
  if (normalized.includes("janelas") || normalized.includes("windows")) {
    return { tool: "desktop.list_windows", arguments: {} };
  }
  if (normalized.includes("área de transferência") || normalized.includes("clipboard")) {
    const writeIntent = /\b(copie|copiar|escreva|escrever|coloque|colocar)\b/iu.test(normalized);
    return writeIntent
      ? { tool: "clipboard.write", arguments: { text: prompt } }
      : { tool: "clipboard.read", arguments: {} };
  }
  if (/\b(abra|abrir|inicie|iniciar)\b/iu.test(normalized)) {
    const program = normalized.includes("discord")
      ? "discord"
      : normalized.includes("visual studio code") || normalized.includes("vscode")
        ? "code"
        : normalized.includes("chrome")
          ? "google-chrome"
          : undefined;
    if (program) return { tool: "apps.launch", arguments: { program, argv: [] } };
  }
  if (normalized.includes("liste os arquivos") || normalized.includes("liste os ficheiros") || normalized.includes("listar arquivos")) {
    return { tool: "files.list", arguments: { root: ".", limit: 200 } };
  }
  if (normalized.includes("procure o arquivo") || normalized.includes("encontre o arquivo") || normalized.includes("buscar arquivo")) {
    return { tool: "files.search", arguments: { root: ".", query: prompt, limit: 200 } };
  }
  if (normalized.includes("crie um arquivo") || normalized.includes("escreva um arquivo")) {
    return {
      tool: "files.write",
      arguments: { path: "./vox-approved-output.txt", content: prompt, overwrite: false },
    };
  }
  return undefined;
}

function fakeFinalAfterTool(message: string): string {
  let result: unknown;
  try {
    result = JSON.parse(message.slice("TOOL_RESULT\n".length));
  } catch {
    return "Recebi o resultado da ferramenta, mas ele não pôde ser interpretado.";
  }
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return "Recebi o resultado da ferramenta.";
  }
  const value = result as { tool?: unknown; status?: unknown };
  const tool = typeof value.tool === "string" ? value.tool : "ferramenta";
  const status = typeof value.status === "string" ? value.status : "desconhecido";
  return `A ferramenta ${tool} terminou com status ${status}.`;
}

export interface OpenAICompatibleOptions {
 baseUrl: string;
 model: string;
 apiKey?: string;
 contextChars?: number;
 maxOutputChars?: number;
}

function extractOpenAIContent(value: unknown, delta: boolean): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const choices = (value as { choices?: unknown }).choices;
  if (!Array.isArray(choices) || !choices[0] || typeof choices[0] !== "object") return undefined;
  const choice = choices[0] as { delta?: unknown; message?: unknown };
  const source = delta ? choice.delta : choice.message;
  if (!source || typeof source !== "object" || Array.isArray(source)) return undefined;
  const content = (source as { content?: unknown }).content;
  return typeof content === "string" ? content : undefined;
}

export class OpenAICompatibleTextProvider implements TextProvider {
  readonly id = "openai-compatible";
  readonly model_ref: string;
  // This adapter currently sends text-only requests. Tool selection remains
  // conservative and broker-validated in agent-core; do not overclaim native
  // provider tool or JSON-mode support in the handshake.
  readonly capabilities: ProviderCapabilities;

  constructor(private readonly options: OpenAICompatibleOptions) {
    validateEndpoint(options.baseUrl);
    this.model_ref = options.model;
    this.capabilities = {
      streaming: true,
      native_tools: false,
      json_mode: false,
      multimodal: false,
      context_chars: options.contextChars ?? 32 * 1024,
      max_output_chars: options.maxOutputChars ?? 128 * 1024,
    };
  }

  async *stream(input: ProviderInput, signal: AbortSignal): AsyncIterable<string> {
    const messages = normalizeInput(input);
    const url = `${this.options.baseUrl.replace(/\/$/u, "")}/chat/completions`;
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (this.options.apiKey) headers.authorization = `Bearer ${this.options.apiKey}`;
    let response: Response;
    try {
      response = await fetch(url, {
        method: "POST",
        headers,
        signal,
        body: JSON.stringify({ model: this.options.model, stream: true, messages }),
      });
    } catch (error) {
      throw transportError(error, signal, "provider request failed");
    }
    if (!response.ok) {
      const body = await response.text().catch(() => "");
      const code = response.status === 401 || response.status === 403 ? "AUTH_ERROR" : response.status === 429 ? "RATE_LIMIT" : "PROVIDER_ERROR";
      const detail = redactTextForModel(body.slice(0, 240));
      throw new ProviderError(code, `provider returned ${response.status}${detail ? `: ${detail}` : ""}`, response.status === 429 || response.status >= 500);
   }
    if (!response.body) throw new ProviderError("EMPTY_STREAM", "provider returned no stream", true);
    const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
    if (!contentType.includes("text/event-stream")) {
      const body = await response.text().catch(() => "");
      let parsed: unknown;
      try {
        parsed = JSON.parse(body);
      } catch {
        throw new ProviderError("INVALID_RESPONSE", "provider returned invalid JSON", false);
      }
      const content = extractOpenAIContent(parsed, false);
      if (content === undefined) throw new ProviderError("INVALID_RESPONSE", "provider response did not contain text content", false);
      yield content;
      return;
    }
    let reader: ReadableStreamDefaultReader<Uint8Array>;
    try {
      reader = response.body.getReader();
    } catch (error) {
      throw transportError(error, signal, "provider stream could not be read");
    }
    const decoder = new TextDecoder();
    let buffer = "";
    const consumeLine = (line: string): { done: true } | { content?: string } | undefined => {
      const trimmed = line.trim();
      if (!trimmed.startsWith("data:")) return undefined;
      const payload = trimmed.slice(5).trim();
      if (payload === "[DONE]") return { done: true };
      try {
        const content = extractOpenAIContent(JSON.parse(payload), true);
        return { content };
      } catch {
        throw new ProviderError("INVALID_STREAM", "provider emitted invalid JSON", false);
      }
    };
    while (true) {
      let next: ReadableStreamReadResult<Uint8Array>;
      try {
        next = await reader.read();
      } catch (error) {
        throw transportError(error, signal, "provider stream read failed");
      }
      if (next.done) break;
      buffer += decoder.decode(next.value, { stream: true });
      if (buffer.length > MAX_UNTERMINATED_SSE_BUFFER_CHARS) {
        throw new ProviderError("INVALID_STREAM", "provider emitted an SSE event that exceeds the size limit", false);
      }
      const lines = buffer.split(/\r?\n/u);
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        const event = consumeLine(line);
        if (event && "done" in event && event.done) return;
        if (event && "content" in event && typeof event.content === "string") yield event.content;
      }
    }
    buffer += decoder.decode();
    if (buffer.length > MAX_UNTERMINATED_SSE_BUFFER_CHARS) {
      throw new ProviderError("INVALID_STREAM", "provider emitted an SSE event that exceeds the size limit", false);
    }
    if (buffer.trim()) {
      const event = consumeLine(buffer);
      if (event && "done" in event && event.done) return;
      if (event && "content" in event && typeof event.content === "string") yield event.content;
    }
  }
}

export function providerFromEnvironment(env: NodeJS.ProcessEnv = process.env): TextProvider {
  if (env.VOX_PROVIDER === "openai-compatible") {
    const baseUrl = env.VOX_PROVIDER_BASE_URL;
    const model = env.VOX_PROVIDER_MODEL;
    if (!baseUrl || !model) throw new ProviderError("CONFIG_ERROR", "VOX_PROVIDER_BASE_URL and VOX_PROVIDER_MODEL are required", false);
    return new OpenAICompatibleTextProvider({
      baseUrl,
      model,
      apiKey: env.VOX_PROVIDER_API_KEY,
      contextChars: positiveEnvironmentNumber(env.VOX_PROVIDER_CONTEXT_CHARS, 32 * 1024),
      maxOutputChars: positiveEnvironmentNumber(env.VOX_PROVIDER_MAX_OUTPUT_CHARS, 128 * 1024),
    });
  }
  if (env.VOX_PROVIDER === "fake-tools") return new FakeTextProvider(true);
  return new FakeTextProvider();
}

function positiveEnvironmentNumber(value: string | undefined, fallback: number): number {
  if (!value) return fallback;
  const parsed = Number.parseInt(value, 10);
  return Number.isSafeInteger(parsed) && parsed > 0 ? Math.min(parsed, 4 * 1024 * 1024) : fallback;
}

/**
 * Probe a compatible endpoint without treating an HTTP response as a model
 * capability claim. Only bounded model ids are returned; response bodies and
 * credentials never enter the error message.
 */
export async function probeOpenAICompatible(
  options: OpenAICompatibleOptions,
  timeoutMs = 3_000,
  signal?: AbortSignal,
): Promise<ProviderProbeResult> {
  validateEndpoint(options.baseUrl);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort("PROBE_TIMEOUT"), Math.max(100, timeoutMs));
  const onAbort = () => controller.abort(signal?.reason);
  signal?.addEventListener("abort", onAbort, { once: true });
  try {
    const headers: Record<string, string> = {};
    if (options.apiKey) headers.authorization = `Bearer ${options.apiKey}`;
    const response = await fetch(`${options.baseUrl.replace(/\/$/u, "")}/models`, {
      method: "GET",
      headers,
      signal: controller.signal,
    });
    if (!response.ok) {
      return {
        available: false,
        model_ids: [],
        status: response.status,
        error: response.status === 401 || response.status === 403 ? "autenticação recusada" : `endpoint respondeu ${response.status}`,
      };
    }
    const body = await response.text();
    if (body.length > 1024 * 1024) return { available: false, model_ids: [], error: "resposta de capabilities excedeu o limite" };
    let parsed: unknown;
    try {
      parsed = JSON.parse(body);
    } catch {
      return { available: false, model_ids: [], error: "endpoint retornou JSON inválido" };
    }
    const entries = parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as { data?: unknown }).data
      : undefined;
    if (!Array.isArray(entries)) return { available: false, model_ids: [], error: "endpoint não retornou uma lista de modelos" };
    const model_ids = entries
      .map((entry) => entry && typeof entry === "object" && typeof (entry as { id?: unknown }).id === "string" ? (entry as { id: string }).id : undefined)
      .filter((id): id is string => Boolean(id && id.length <= 128))
      .slice(0, 64);
    return { available: true, model_ids };
  } catch (error) {
    const message = controller.signal.reason === "PROBE_TIMEOUT"
      ? "probe excedeu o tempo limite"
      : signal?.aborted
        ? "probe cancelado"
        : "endpoint indisponível";
    return { available: false, model_ids: [], error: message };
  } finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", onAbort);
  }
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isJsonValue(value: unknown, depth = 0): value is JsonValue {
  if (depth > 32 || value === null || typeof value === "string" || typeof value === "boolean") return depth <= 32;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value)) return value.every((item) => isJsonValue(item, depth + 1));
  if (!isObject(value)) return false;
  return Object.values(value).every((item) => isJsonValue(item, depth + 1));
}

function onlyKeys(object: Record<string, unknown>, keys: readonly string[]): boolean {
  const allowed = new Set(keys);
  return Object.keys(object).every((key) => allowed.has(key));
}

function validToolCall(value: unknown): NormalizedToolCall | undefined {
  if (!isObject(value) || !onlyKeys(value, ["tool", "arguments"])) return undefined;
  if (typeof value.tool !== "string" || !/^[a-z][a-z0-9_.-]{0,127}$/u.test(value.tool)) return undefined;
  if (!isObject(value.arguments) || !isJsonValue(value.arguments)) return undefined;
  if (Buffer.byteLength(JSON.stringify(value.arguments), "utf8") > MAX_TOOL_ARGUMENT_BYTES) return undefined;
  return { tool: value.tool, arguments: value.arguments };
}

/**
 * Parse the only model output format accepted by the text-only agent loop.
 * Some otherwise compatible models wrap a JSON-only answer in one Markdown
 * fence, so that exact wrapper is tolerated. Extra fields, malformed calls
 * and prose outside the fence remain invalid and never become an action.
 */
export function parseModelDecision(text: string): ModelDecision | undefined {
  const normalized = text.trim();
  const fenced = /^```(?:json)?[ \t]*\r?\n([\s\S]*?)\r?\n```$/iu.exec(normalized);
  const candidate = fenced ? fenced[1]?.trim() ?? "" : normalized;
  let value: unknown;
  try {
    value = JSON.parse(candidate);
  } catch {
    return undefined;
  }
  if (!isObject(value) || typeof value.type !== "string") return undefined;

  if (value.type === "tool_call") {
    if (!onlyKeys(value, ["type", "tool", "arguments"])) return undefined;
    const call = validToolCall({ tool: value.tool, arguments: value.arguments });
    return call ? { type: "tool_calls", calls: [call] } : undefined;
  }
  if (value.type === "tool_calls") {
    if (!onlyKeys(value, ["type", "calls"]) || !Array.isArray(value.calls)) return undefined;
    if (value.calls.length === 0 || value.calls.length > MAX_TOOL_CALLS_PER_DECISION) return undefined;
    const calls = value.calls.map((call) => validToolCall(call));
    return calls.every((call): call is NormalizedToolCall => call !== undefined)
      ? { type: "tool_calls", calls }
      : undefined;
  }
  if (value.type === "final") {
    if (!onlyKeys(value, ["type", "content"])) return undefined;
    if (typeof value.content !== "string" || value.content.length === 0 || value.content.length > MAX_FINAL_CONTENT_CHARS) return undefined;
    return { type: "final", content: value.content };
  }
  return undefined;
}

/**
 * Kept as a narrow utility for callers that only need to validate one raw
 * call. Agent-core uses parseModelDecision so prose cannot become a call.
 */
export function parseJsonToolCall(text: string): NormalizedToolCall | undefined {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return undefined;
  }
  return validToolCall(value);
}
