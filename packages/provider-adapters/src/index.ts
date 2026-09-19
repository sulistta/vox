import type { EffectClass, JsonObject, JsonValue } from "@vox/protocol";

export type ProviderRole = "system" | "user" | "assistant";
export interface ProviderMessage { role: ProviderRole; content: string; }
export interface ProviderProbeResult { available: boolean; model_ids: string[]; status?: number; error?: string; }

export class ProviderError extends Error {
  constructor(readonly code: string, message: string, readonly retryable: boolean) {
    super(message); this.name = "ProviderError";
  }
}

export interface TaskSpec {
  objective: string;
  app_hints: string[];
  targets: string[];
  content_instruction?: string;
  expected_result: string;
}

export interface ActionCandidate {
  id: string;
  tool: string;
  arguments: JsonObject;
  label: string;
  effect: EffectClass;
  verification: string;
}

export interface ToolDescription { name: string; effect: EffectClass; arguments: string; }
export interface CandidateRequest { task: TaskSpec; state: string; tools: readonly ToolDescription[]; }
export interface ComposeRequest { instruction: string; state: string; }

/** The text service can create data only. It can never execute a tool. */
export interface TextPlanner {
  readonly id: "openai-compatible-text";
  readonly model_ref: string;
  readonly context_chars: number;
  interpretTask(request: string, signal: AbortSignal): Promise<TaskSpec>;
  proposeCandidates(request: CandidateRequest, signal: AbortSignal): Promise<ActionCandidate[]>;
  compose(request: ComposeRequest, signal: AbortSignal): Promise<string>;
  probe(signal: AbortSignal): Promise<ProviderProbeResult>;
}

export interface ChoiceOption { id: string; description: string; }
export interface ChoiceQuestion { type: "choice"; id: string; instructions: string; options: readonly ChoiceOption[]; }
export interface ScoreQuestion { type: "score"; id: string; instructions: string; levels: readonly ChoiceOption[]; }
export interface NoulQuestion { type: "noul"; id: string; statement: string; }
export type DecisionQuestion = ChoiceQuestion | ScoreQuestion | NoulQuestion;
export interface DecisionInput { state: string; questions: readonly DecisionQuestion[]; }
export interface ChoiceAnswer { type: "choice"; choice: string; confidence: number; probabilities: Readonly<Record<string, number>>; }
export interface ScoreAnswer { type: "score"; score: number; confidence: number; probabilities: Readonly<Record<string, number>>; }
export interface NoulAnswer { type: "noul"; noul: number; }
export type DecisionAnswer = ChoiceAnswer | ScoreAnswer | NoulAnswer;
export interface DecisionResponse { model_ref: string; answers: Readonly<Record<string, DecisionAnswer>>; }

export interface AgentProvider {
  readonly id: "typesafe-jev";
  readonly model_ref: string;
  readonly context_chars: number;
  decide(input: DecisionInput, signal: AbortSignal): Promise<DecisionResponse>;
}

/** Descriptive vocabulary only; Rust validates every candidate at execution. */
export const TOOL_CATALOG: readonly ToolDescription[] = [
  { name: "desktop.list_windows", effect: "read", arguments: "{}" },
  { name: "desktop.snapshot", effect: "read", arguments: "{app_pid?: positive integer}" },
  { name: "desktop.query", effect: "read", arguments: "{snapshot_id: string, query: selector}" },
  { name: "desktop.wait_for", effect: "read", arguments: "{query: selector, timeout_ms?: positive integer, app_pid?: positive integer}" },
  { name: "desktop.act", effect: "write", arguments: "{snapshot_id: string, element_ref: string, action: string, value?: string}" },
  { name: "clipboard.read", effect: "read", arguments: "{}" },
  { name: "clipboard.write", effect: "write", arguments: "{text: string}" },
  { name: "files.search", effect: "read", arguments: "{root?, query, limit?}" },
  { name: "files.list", effect: "read", arguments: "{root?, limit?}" },
  { name: "files.read", effect: "read", arguments: "{path}" },
  { name: "files.write", effect: "write", arguments: "{path, content, overwrite?}" },
  { name: "files.move", effect: "write", arguments: "{source, destination, overwrite?}" },
  { name: "process.list", effect: "read", arguments: "{limit?}" },
  { name: "process.terminate", effect: "external", arguments: "{pid, command?, start_time?}" },
  { name: "apps.resolve", effect: "read", arguments: "{name}" },
  { name: "apps.launch", effect: "external", arguments: "{program, argv?}" },
  { name: "paths.open", effect: "external", arguments: "{path}" },
  { name: "shell.exec", effect: "arbitrary", arguments: "{program, argv?, cwd?, timeout_ms?}" },
] as const;

const EFFECTS = new Map(TOOL_CATALOG.map((tool) => [tool.name, tool.effect]));
const TYPESAFE_SYSTEM_ONE_URL = "https://api.typesafe.ai/v1/systemone";
const DEFAULT_JEV_MODEL = "jev-latest";
const MAX_STATE_CHARS = 64 * 1024;
const MAX_RESPONSE_CHARS = 1024 * 1024;
const MAX_SSE_BUFFER_CHARS = 256 * 1024;
const identifier = /^[a-z][a-z0-9_.-]{0,127}$/u;

export function effectForTool(tool: string): EffectClass { return EFFECTS.get(tool) ?? "arbitrary"; }

export function redactTextForModel(value: string): string {
  return value
    .replace(/Bearer\s+[^\s,}]+/giu, "Bearer [REDACTED]")
    .replace(/\bsk-[a-z0-9_-]{16,}\b/giu, "[REDACTED]")
    .replace(/(["']?(?:access[_-]?token|api[_-]?key|authorization|client[_-]?secret|cookie|credential(?:s)?|credencial|credenciais|pass(?:word|phrase)?|private[_-]?key|refresh[_-]?token|secret|token|senha(?:s)?|segredo(?:s)?|chave(?:s)?)["']?\s*[:=]\s*)(?:"[^"]*"|'[^']*'|[^\s,}]+)/giu, "$1[REDACTED]");
}

const sensitiveKey = /(?:access[_-]?token|api[_-]?key|authorization|client[_-]?secret|cookie|credential(?:s)?|credencial|credenciais|pass(?:word|phrase)?|private[_-]?key|refresh[_-]?token|secret|token|senha(?:s)?|segredo(?:s)?|chave(?:s)?)/iu;
export function redactJsonForModel(value: JsonValue): JsonValue {
  if (typeof value === "string") return redactTextForModel(value);
  if (value === null || typeof value === "number" || typeof value === "boolean") return value;
  if (Array.isArray(value)) return value.map(redactJsonForModel);
  return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, sensitiveKey.test(key) ? "[REDACTED]" : redactJsonForModel(item)]));
}

function isObject(value: unknown): value is Record<string, unknown> { return typeof value === "object" && value !== null && !Array.isArray(value); }
function isJsonValue(value: unknown, depth = 0): value is JsonValue {
  if (depth > 32) return false;
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (Array.isArray(value)) return value.every((item) => isJsonValue(item, depth + 1));
  return isObject(value) && Object.values(value).every((item) => isJsonValue(item, depth + 1));
}

function validateEndpoint(baseUrl: string): void {
  let parsed: URL;
  try { parsed = new URL(baseUrl); } catch { throw new ProviderError("CONFIG_ERROR", "endpoint inválido", false); }
  const local = ["localhost", "127.0.0.1", "::1"].includes(parsed.hostname);
  if (parsed.protocol !== "https:" && !(local && parsed.protocol === "http:")) throw new ProviderError("CONFIG_ERROR", "endpoint deve usar HTTPS; HTTP é permitido somente em localhost", false);
  if (parsed.username || parsed.password) throw new ProviderError("CONFIG_ERROR", "endpoint não pode conter credenciais", false);
}

function normalizeMessages(messages: readonly ProviderMessage[]): ProviderMessage[] {
  if (messages.length === 0 || messages.length > 32) throw new ProviderError("INVALID_REQUEST", "mensagens inválidas", false);
  return messages.map((message) => {
    if (!message || !["system", "user", "assistant"].includes(message.role) || typeof message.content !== "string" || message.content.length > MAX_RESPONSE_CHARS) throw new ProviderError("INVALID_REQUEST", "mensagem inválida", false);
    return { role: message.role, content: redactTextForModel(message.content) };
  });
}

function transportError(error: unknown, signal: AbortSignal, fallback: string): ProviderError {
  if (signal.aborted) return new ProviderError("CANCELLED", "requisição cancelada", false);
  return new ProviderError("NETWORK_ERROR", redactTextForModel(error instanceof Error ? error.message : fallback), true);
}

function responseError(service: string, status: number, body: string): ProviderError {
  const code = status === 401 || status === 403 ? "AUTH_ERROR" : status === 429 ? "RATE_LIMIT" : "PROVIDER_ERROR";
  const detail = redactTextForModel(body.slice(0, 240));
  return new ProviderError(code, `${service} respondeu ${status}${detail ? `: ${detail}` : ""}`, status === 429 || status >= 500);
}

function extractContent(value: unknown, delta: boolean): string | undefined {
  if (!isObject(value) || !Array.isArray(value.choices) || !isObject(value.choices[0])) return undefined;
  const choice = value.choices[0];
  const source = delta ? choice.delta : choice.message;
  return isObject(source) && typeof source.content === "string" ? source.content : undefined;
}

export interface OpenAICompatibleTextOptions { baseUrl: string; model: string; apiKey: string; contextChars?: number; }

export class OpenAICompatibleTextPlanner implements TextPlanner {
  readonly id = "openai-compatible-text" as const;
  readonly model_ref: string;
  readonly context_chars: number;

  constructor(private readonly options: OpenAICompatibleTextOptions) {
    validateEndpoint(options.baseUrl);
    if (!options.model.trim() || !options.apiKey.trim()) throw new ProviderError("CONFIG_ERROR", "modelo e chave de texto são obrigatórios", false);
    this.model_ref = options.model.trim();
    this.context_chars = Math.min(Math.max(options.contextChars ?? 32 * 1024, 1024), MAX_STATE_CHARS);
  }

  async interpretTask(request: string, signal: AbortSignal): Promise<TaskSpec> {
    const raw = await this.completeJson([
      { role: "system", content: "Extraia uma especificação de tarefa. Responda SOMENTE JSON com objective, app_hints, targets, content_instruction opcional e expected_result. Não proponha ferramentas nem execute ações." },
      { role: "user", content: request },
    ], signal);
    return parseTaskSpec(raw);
  }

  async proposeCandidates(request: CandidateRequest, signal: AbortSignal): Promise<ActionCandidate[]> {
    const messages: ProviderMessage[] = [
      { role: "system", content: 'Proponha somente candidatos para o próximo passo. Responda SOMENTE JSON válido, sem campos extras. Formato: {"candidates":[{"id":"inspect","tool":"desktop.list_windows","arguments":{},"label":"Listar janelas","verification":"Lista de janelas observada"}]}. Cada candidato deve conter exatamente id, tool, arguments, label e verification. id deve ser único e corresponder a ^[a-z][a-z0-9_.-]{0,127}$; não use reobserve, replan, ask_user ou finish. arguments é um objeto JSON; label e verification são textos não vazios de até 512 e 1024 caracteres. Retorne entre 1 e 16 candidatos. Não inclua effect: ele é calculado pelo runtime. Você não executa ferramentas. Use somente o catálogo entregue.' },
      { role: "user", content: JSON.stringify({ task: request.task, state: request.state, tools: request.tools }) },
    ];
    for (let attempt = 0; ; attempt += 1) {
      try {
        return parseCandidates(await this.completeJson(messages, signal));
      } catch (error) {
        if (!(error instanceof ProviderError) || error.code !== "INVALID_RESPONSE" || attempt >= 1 || signal.aborted) throw error;
        messages.push({ role: "user", content: `A resposta anterior foi rejeitada: ${error.message}. Gere novamente o objeto completo conforme o formato exigido. Nenhuma ação foi executada.` });
      }
    }
  }

  async compose(request: ComposeRequest, signal: AbortSignal): Promise<string> {
    return this.complete([
      { role: "system", content: "Produza somente o conteúdo pedido, sem explicar o processo nem tomar decisões de ferramenta." },
      { role: "user", content: JSON.stringify(request) },
    ], signal);
  }

  async probe(signal: AbortSignal): Promise<ProviderProbeResult> {
    return probeOpenAICompatible(this.options, 3_000, signal);
  }

  private async completeJson(messages: readonly ProviderMessage[], signal: AbortSignal): Promise<unknown> {
    const text = await this.complete(messages, signal);
    try { return JSON.parse(stripFence(text)); } catch { throw new ProviderError("INVALID_RESPONSE", "provider de texto não retornou JSON válido", false); }
  }

  private async complete(messages: readonly ProviderMessage[], signal: AbortSignal): Promise<string> {
    const normalized = normalizeMessages(messages);
    let response: Response;
    try {
      response = await fetch(`${this.options.baseUrl.replace(/\/$/u, "")}/chat/completions`, {
        method: "POST", headers: { "content-type": "application/json", authorization: `Bearer ${this.options.apiKey}` }, signal,
        body: JSON.stringify({ model: this.model_ref, stream: true, messages: normalized }),
      });
    } catch (error) { throw transportError(error, signal, "provider de texto indisponível"); }
    if (!response.ok) throw responseError("provider de texto", response.status, await response.text().catch(() => ""));
    if (!response.body) throw new ProviderError("EMPTY_STREAM", "provider de texto não retornou conteúdo", true);
    const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
    if (!contentType.includes("text/event-stream")) {
      let parsed: unknown;
      try { parsed = JSON.parse(await response.text()); } catch { throw new ProviderError("INVALID_RESPONSE", "provider de texto retornou JSON inválido", false); }
      const content = extractContent(parsed, false);
      if (content === undefined) throw new ProviderError("INVALID_RESPONSE", "provider de texto não retornou conteúdo", false);
      return boundedContent(content);
    }
    let output = "";
    let buffer = "";
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    const consume = (line: string): boolean => {
      const trimmed = line.trim();
      if (!trimmed.startsWith("data:")) return false;
      const payload = trimmed.slice(5).trim();
      if (payload === "[DONE]") return true;
      let parsed: unknown;
      try { parsed = JSON.parse(payload); } catch { throw new ProviderError("INVALID_STREAM", "provider de texto emitiu SSE inválido", false); }
      output += extractContent(parsed, true) ?? "";
      if (output.length > MAX_RESPONSE_CHARS) throw new ProviderError("CONTEXT_LIMIT", "resposta de texto excede o limite", false);
      return false;
    };
    try {
      while (true) {
        const next = await reader.read();
        if (next.done) break;
        buffer += decoder.decode(next.value, { stream: true });
        if (buffer.length > MAX_SSE_BUFFER_CHARS) throw new ProviderError("INVALID_STREAM", "evento SSE excede o limite", false);
        const lines = buffer.split(/\r?\n/u);
        buffer = lines.pop() ?? "";
        if (lines.some(consume)) break;
      }
      if (buffer.trim()) consume(buffer + decoder.decode());
    } catch (error) {
      if (error instanceof ProviderError) throw error;
      throw transportError(error, signal, "leitura do provider de texto falhou");
    }
    return boundedContent(output);
  }
}

function boundedContent(value: string): string {
  const redacted = redactTextForModel(value).trim();
  if (!redacted) throw new ProviderError("INVALID_RESPONSE", "provider de texto retornou conteúdo vazio", false);
  if (redacted.length > MAX_RESPONSE_CHARS) throw new ProviderError("CONTEXT_LIMIT", "resposta de texto excede o limite", false);
  return redacted;
}
function stripFence(value: string): string { const match = /^```(?:json)?\s*\n([\s\S]*?)\n```$/iu.exec(value.trim()); return match?.[1]?.trim() ?? value.trim(); }
function stringArray(value: unknown, limit: number): string[] | undefined { return Array.isArray(value) && value.length <= limit && value.every((item) => typeof item === "string" && item.trim() && item.length <= 512) ? value.map((item) => (item as string).trim()) : undefined; }

function parseTaskSpec(value: unknown): TaskSpec {
  if (!isObject(value) || Object.keys(value).some((key) => !["objective", "app_hints", "targets", "content_instruction", "expected_result"].includes(key))) throw new ProviderError("INVALID_RESPONSE", "especificação de tarefa inválida", false);
  const app_hints = stringArray(value.app_hints, 16); const targets = stringArray(value.targets, 32);
  if (typeof value.objective !== "string" || !value.objective.trim() || value.objective.length > 2048 || typeof value.expected_result !== "string" || !value.expected_result.trim() || value.expected_result.length > 2048 || !app_hints || !targets || (value.content_instruction !== undefined && (typeof value.content_instruction !== "string" || value.content_instruction.length > 16 * 1024))) throw new ProviderError("INVALID_RESPONSE", "especificação de tarefa inválida", false);
  return { objective: value.objective.trim(), app_hints, targets, ...(typeof value.content_instruction === "string" && value.content_instruction.trim() ? { content_instruction: value.content_instruction.trim() } : {}), expected_result: value.expected_result.trim() };
}

function parseCandidates(value: unknown): ActionCandidate[] {
  if (!isObject(value) || !Array.isArray(value.candidates) || value.candidates.length === 0 || value.candidates.length > 16) throw new ProviderError("INVALID_RESPONSE", "candidatos de ação inválidos", false);
  const ids = new Set<string>();
  return value.candidates.map((candidate, index) => {
    const invalid = (field: string): never => { throw new ProviderError("INVALID_RESPONSE", `candidato de ação inválido: candidates[${index}].${field}`, false); };
    if (!isObject(candidate)) return invalid("objeto esperado");
    if (Object.keys(candidate).some((key) => !["id", "tool", "arguments", "label", "verification"].includes(key))) return invalid("campos extras não permitidos");
    if (typeof candidate.id !== "string" || !identifier.test(candidate.id) || ids.has(candidate.id) || ["reobserve", "replan", "ask_user", "finish"].includes(candidate.id)) return invalid("id deve ser único, não reservado e seguir o formato exigido");
    if (typeof candidate.tool !== "string" || !EFFECTS.has(candidate.tool)) return invalid("tool deve pertencer ao catálogo");
    if (!isObject(candidate.arguments) || !isJsonValue(candidate.arguments)) return invalid("arguments deve ser um objeto JSON");
    if (typeof candidate.label !== "string" || !candidate.label.trim() || candidate.label.length > 512) return invalid("label deve ser texto de 1 a 512 caracteres");
    if (typeof candidate.verification !== "string" || !candidate.verification.trim() || candidate.verification.length > 1024) return invalid("verification deve ser texto de 1 a 1024 caracteres");
    ids.add(candidate.id);
    return { id: candidate.id, tool: candidate.tool, arguments: candidate.arguments, label: candidate.label.trim(), effect: effectForTool(candidate.tool), verification: candidate.verification.trim() };
  });
}

export interface TypeSafeJevOptions { apiKey: string; model?: string; baseUrl?: string; timeoutMs?: number; }
function validateDecisionInput(input: DecisionInput): void {
  if (typeof input.state !== "string" || !input.state.trim() || input.state.length > MAX_STATE_CHARS || !Array.isArray(input.questions) || input.questions.length === 0 || input.questions.length > 32) throw new ProviderError("INVALID_REQUEST", "perguntas Jev inválidas", false);
  const ids = new Set<string>();
  for (const question of input.questions) {
    if (!identifier.test(question.id) || ids.has(question.id)) throw new ProviderError("INVALID_REQUEST", "id de pergunta Jev inválido", false);
    ids.add(question.id);
    if (question.type === "noul") { if (!question.statement.trim() || question.statement.length > 4096) throw new ProviderError("INVALID_REQUEST", "pergunta Noul inválida", false); continue; }
    const values = question.type === "choice" ? question.options : question.levels;
    if (!question.instructions.trim() || values.length < 2 || values.length > 255 || values.some((option: ChoiceOption) => !identifier.test(option.id) || !option.description.trim() || option.description.length > 512)) throw new ProviderError("INVALID_REQUEST", "pergunta Jev inválida", false);
  }
}
function parseProbabilities(raw: unknown, options: readonly ChoiceOption[], questionId: string): Readonly<Record<string, number>> {
  if (!isObject(raw)) throw new ProviderError("INVALID_RESPONSE", `probabilidades inválidas para ${questionId}`, false);
  const probabilities: Record<string, number> = {}; let sum = 0;
  for (const option of options) { const probability = raw[option.id]; if (typeof probability !== "number" || !Number.isFinite(probability) || probability < 0 || probability > 1) throw new ProviderError("INVALID_RESPONSE", `probabilidades inválidas para ${questionId}`, false); probabilities[option.id] = probability; sum += probability; }
  if (Object.keys(raw).some((key) => !options.some((option) => option.id === key)) || Math.abs(sum - 1) > 0.01) throw new ProviderError("INVALID_RESPONSE", `probabilidades inválidas para ${questionId}`, false);
  return probabilities;
}
function parseDecisionResponse(value: unknown, input: DecisionInput, fallback: string): DecisionResponse {
  if (!isObject(value) || !isObject(value.answers)) throw new ProviderError("INVALID_RESPONSE", "Jev não retornou respostas", false);
  const model_ref = typeof value.model === "string" && value.model.length <= 128 && value.model ? value.model : fallback;
  const answers: Record<string, DecisionAnswer> = {};
  for (const question of input.questions) {
    const raw = value.answers[question.id];
    if (!isObject(raw) || raw.type !== question.type) throw new ProviderError("INVALID_RESPONSE", `resposta Jev inválida para ${question.id}`, false);
    if (question.type === "noul") { if (typeof raw.noul !== "number" || !Number.isFinite(raw.noul) || raw.noul < 0 || raw.noul > 1) throw new ProviderError("INVALID_RESPONSE", `Noul inválido para ${question.id}`, false); answers[question.id] = { type: "noul", noul: raw.noul }; continue; }
    if (typeof raw.confidence !== "number" || !Number.isFinite(raw.confidence) || raw.confidence < 0 || raw.confidence > 1) throw new ProviderError("INVALID_RESPONSE", `resposta Jev inválida para ${question.id}`, false);
    if (question.type === "choice") {
      if (typeof raw.choice !== "string" || !question.options.some((option) => option.id === raw.choice)) throw new ProviderError("INVALID_RESPONSE", `resposta Jev inválida para ${question.id}`, false);
      answers[question.id] = { type: "choice", choice: raw.choice, confidence: raw.confidence, probabilities: parseProbabilities(raw.probabilities, question.options, question.id) };
      continue;
    }
    const scoreOptions = question.levels.map((level, index) => ({ id: String(index), description: level.description }));
    if (typeof raw.score !== "number" || !Number.isFinite(raw.score) || raw.score < 0 || raw.score > question.levels.length - 1) throw new ProviderError("INVALID_RESPONSE", `score inválido para ${question.id}`, false);
    answers[question.id] = { type: "score", score: raw.score, confidence: raw.confidence, probabilities: parseProbabilities(raw.probabilities, scoreOptions, question.id) };
  }
  return { model_ref, answers };
}

export class TypeSafeJevAgent implements AgentProvider {
  readonly id = "typesafe-jev" as const;
  readonly model_ref: string;
  readonly context_chars = MAX_STATE_CHARS;
  private readonly baseUrl: string;
  private readonly timeoutMs: number;
  constructor(private readonly options: TypeSafeJevOptions) { this.baseUrl = options.baseUrl ?? TYPESAFE_SYSTEM_ONE_URL; validateEndpoint(this.baseUrl); if (!options.apiKey.trim()) throw new ProviderError("CONFIG_ERROR", "chave Jev obrigatória", false); this.model_ref = options.model?.trim() || DEFAULT_JEV_MODEL; this.timeoutMs = Math.min(Math.max(options.timeoutMs ?? 10_000, 100), 60_000); }
  async decide(input: DecisionInput, signal: AbortSignal): Promise<DecisionResponse> {
    validateDecisionInput(input);
    if (signal.aborted) throw new ProviderError("CANCELLED", "decisão Jev cancelada", false);
    const controller = new AbortController(); const timeout = setTimeout(() => controller.abort("JEV_TIMEOUT"), this.timeoutMs); const abort = () => controller.abort(signal.reason); signal.addEventListener("abort", abort, { once: true });
    try {
      const questions = Object.fromEntries(input.questions.map((question) => {
        if (question.type === "noul") return [question.id, { type: "noul", instructions: question.statement }];
        if (question.type === "score") return [question.id, { type: "score", instructions: question.instructions, criteria: question.levels.map((level) => level.description) }];
        return [question.id, { type: "choice", instructions: question.instructions, criteria: Object.fromEntries(question.options.map((option) => [option.id, option.description])) }];
      }));
      let response: Response;
      try { response = await fetch(this.baseUrl, { method: "POST", headers: { authorization: `Bearer ${this.options.apiKey}`, "content-type": "application/json" }, signal: controller.signal, body: JSON.stringify({ state: redactTextForModel(input.state), model: this.model_ref, questions }) }); }
      catch (error) { if (signal.aborted) throw new ProviderError("CANCELLED", "decisão Jev cancelada", false); if (controller.signal.reason === "JEV_TIMEOUT") throw new ProviderError("TIMEOUT", "Jev excedeu 10 segundos", true); throw transportError(error, signal, "Jev indisponível"); }
      const body = await response.text().catch(() => ""); if (!response.ok) throw responseError("Jev", response.status, body); if (body.length > MAX_RESPONSE_CHARS) throw new ProviderError("INVALID_RESPONSE", "resposta Jev excede o limite", false);
      let parsed: unknown; try { parsed = JSON.parse(body); } catch { throw new ProviderError("INVALID_RESPONSE", "Jev retornou JSON inválido", false); }
      return parseDecisionResponse(parsed, input, this.model_ref);
    } finally { clearTimeout(timeout); signal.removeEventListener("abort", abort); }
  }
}

export function agentFromEnvironment(env: NodeJS.ProcessEnv = process.env): AgentProvider { return new TypeSafeJevAgent({ apiKey: env.VOX_AGENT_API_KEY ?? "", model: env.VOX_AGENT_MODEL ?? DEFAULT_JEV_MODEL, baseUrl: env.VOX_AGENT_BASE_URL }); }
export function textPlannerFromEnvironment(env: NodeJS.ProcessEnv = process.env): TextPlanner { return new OpenAICompatibleTextPlanner({ baseUrl: env.VOX_TEXT_BASE_URL ?? "", model: env.VOX_TEXT_MODEL ?? "", apiKey: env.VOX_TEXT_API_KEY ?? "" }); }

export async function probeOpenAICompatible(options: OpenAICompatibleTextOptions, timeoutMs = 3_000, signal?: AbortSignal): Promise<ProviderProbeResult> {
  validateEndpoint(options.baseUrl); const controller = new AbortController(); const timeout = setTimeout(() => controller.abort("PROBE_TIMEOUT"), Math.max(100, timeoutMs)); const abort = () => controller.abort(signal?.reason); signal?.addEventListener("abort", abort, { once: true });
  try { const response = await fetch(`${options.baseUrl.replace(/\/$/u, "")}/models`, { headers: { authorization: `Bearer ${options.apiKey}` }, signal: controller.signal }); if (!response.ok) return { available: false, model_ids: [], status: response.status, error: response.status === 401 || response.status === 403 ? "autenticação recusada" : `endpoint respondeu ${response.status}` }; const parsed = JSON.parse(await response.text()) as { data?: unknown }; const model_ids = Array.isArray(parsed.data) ? parsed.data.flatMap((entry) => isObject(entry) && typeof entry.id === "string" && entry.id.length <= 128 ? [entry.id] : []).slice(0, 64) : []; return { available: true, model_ids }; }
  catch { return { available: false, model_ids: [], error: controller.signal.reason === "PROBE_TIMEOUT" ? "teste excedeu o tempo limite" : signal?.aborted ? "teste cancelado" : "endpoint indisponível" }; }
  finally { clearTimeout(timeout); signal?.removeEventListener("abort", abort); }
}
