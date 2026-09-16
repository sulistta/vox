import type { EffectClass, JsonObject } from "@vox/protocol";

export interface ProviderCapabilities {
  streaming: boolean;
  native_tools: boolean;
  json_mode: boolean;
  multimodal: false;
  context_chars: number;
  max_output_chars: number;
}

export interface TextProvider {
  readonly id: string;
  readonly model_ref?: string;
  readonly capabilities: ProviderCapabilities;
  stream(prompt: string, signal: AbortSignal): AsyncIterable<string>;
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

export interface PlannedToolCall extends NormalizedToolCall {
  effect: EffectClass;
}

export class ProviderError extends Error {
  constructor(readonly code: string, message: string, readonly retryable: boolean) {
    super(message);
    this.name = "ProviderError";
  }
}

function redactProviderText(value: string): string {
  return value
    .replace(/Bearer\s+[^\s,}]+/giu, "Bearer [REDACTED]")
    .replace(/(["']?(?:api[_-]?key|token|secret)["']?\s*[:=]\s*["']?)[^"'\s,}]+/giu, "$1[REDACTED]");
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
  readonly id = "fake-text";
  readonly model_ref = "fake-text";
  readonly capabilities = {
    streaming: true,
    native_tools: false,
    json_mode: false,
    multimodal: false,
    context_chars: 16 * 1024,
    max_output_chars: 64 * 1024,
  } as const;

  async *stream(prompt: string, signal: AbortSignal): AsyncIterable<string> {
    const response = prompt.toLowerCase().includes("janelas") || prompt.toLowerCase().includes("windows")
      ? "Vou consultar as janelas abertas e confirmar o resultado."
      : `Recebi: ${prompt}`;
    for (const chunk of response.match(/.{1,18}/gu) ?? [response]) {
      if (signal.aborted) throw new ProviderError("CANCELLED", "provider cancelled", false);
      await new Promise((resolve) => setTimeout(resolve, 3));
      yield chunk;
    }
  }
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

  async *stream(prompt: string, signal: AbortSignal): AsyncIterable<string> {
    const url = `${this.options.baseUrl.replace(/\/$/u, "")}/chat/completions`;
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (this.options.apiKey) headers.authorization = `Bearer ${this.options.apiKey}`;
    let response: Response;
    try {
      response = await fetch(url, {
        method: "POST",
        headers,
        signal,
        body: JSON.stringify({ model: this.options.model, stream: true, messages: [{ role: "user", content: prompt }] }),
      });
    } catch (error) {
      if (signal.aborted) throw new ProviderError("CANCELLED", "provider cancelled", false);
      throw new ProviderError("NETWORK_ERROR", redactProviderText(error instanceof Error ? error.message : "provider request failed"), true);
    }
    if (!response.ok) {
      const body = await response.text().catch(() => "");
      const code = response.status === 401 || response.status === 403 ? "AUTH_ERROR" : response.status === 429 ? "RATE_LIMIT" : "PROVIDER_ERROR";
      const detail = redactProviderText(body.slice(0, 240));
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
    const reader = response.body.getReader();
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
      const next = await reader.read();
      if (next.done) break;
      buffer += decoder.decode(next.value, { stream: true });
      const lines = buffer.split(/\r?\n/u);
      buffer = lines.pop() ?? "";
      for (const line of lines) {
        const event = consumeLine(line);
        if (event && "done" in event && event.done) return;
        if (event && "content" in event && typeof event.content === "string") yield event.content;
      }
    }
    buffer += decoder.decode();
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

export function plannedTool(prompt: string): PlannedToolCall | undefined {
  const normalized = prompt.toLowerCase();
  if (normalized.includes("janelas") || normalized.includes("windows")) {
    return { tool: "desktop.list_windows", arguments: {}, effect: "read" };
  }
  if (normalized.includes("área de transferência") || normalized.includes("clipboard")) {
    const writeIntent = /\b(copie|copiar|escreva|escrever|coloque|colocar)\b/iu.test(normalized);
    return writeIntent
      ? { tool: "clipboard.write", arguments: { text: prompt }, effect: "write" }
      : { tool: "clipboard.read", arguments: {}, effect: "read" };
  }
  if (/\b(abra|abrir|inicie|iniciar)\b/iu.test(normalized)) {
    const program = normalized.includes("discord")
      ? "discord"
      : normalized.includes("visual studio code") || normalized.includes("vscode")
        ? "code"
        : normalized.includes("chrome")
          ? "google-chrome"
          : undefined;
    if (program) return { tool: "apps.launch", arguments: { program, argv: [] }, effect: "external" };
  }
  if (normalized.includes("liste os arquivos") || normalized.includes("liste os ficheiros") || normalized.includes("listar arquivos")) {
    return { tool: "files.list", arguments: { root: ".", limit: 200 }, effect: "read" };
  }
  if (normalized.includes("procure o arquivo") || normalized.includes("encontre o arquivo") || normalized.includes("buscar arquivo")) {
    return { tool: "files.search", arguments: { root: ".", query: prompt, limit: 200 }, effect: "read" };
  }
  if (normalized.includes("crie um arquivo") || normalized.includes("escreva um arquivo")) {
    return {
      tool: "files.write",
      arguments: { path: "./vox-approved-output.txt", content: prompt, overwrite: false },
      effect: "write",
    };
  }
  return undefined;
}

export function toolArguments(prompt: string): JsonObject | undefined {
  return plannedTool(prompt)?.arguments;
}

/**
 * Validate the conservative JSON fallback accepted from models without
 * native tool calling. Invalid or extra-shaped output is data, never an
 * executable action. The broker still revalidates the resulting call.
 */
export function parseJsonToolCall(text: string): NormalizedToolCall | undefined {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return undefined;
  }
  if (!value || typeof value !== "object" || Array.isArray(value)) return undefined;
  const object = value as Record<string, unknown>;
  if (typeof object.tool !== "string" || !/^[a-z][a-z0-9_.-]{0,127}$/u.test(object.tool)) return undefined;
  if (!object.arguments || typeof object.arguments !== "object" || Array.isArray(object.arguments)) return undefined;
  return { tool: object.tool, arguments: object.arguments as JsonObject };
}
