import { randomUUID } from "node:crypto";
import {
  MAX_MESSAGE_BYTES,
  PROTOCOL_VERSION,
  ProtocolError,
  encodeMessage,
  parseMessage,
  type IpcMessage,
  type JsonValue,
  type ToolResultMessage,
} from "@vox/protocol";
import {
  ProviderError,
  TOOL_CATALOG,
  agentFromEnvironment,
  effectForTool,
  redactJsonForModel,
  redactTextForModel,
  textPlannerFromEnvironment,
  type ActionCandidate,
  type AgentProvider,
  type DecisionAnswer,
  type TaskSpec,
  type TextPlanner,
} from "@vox/provider-adapters";
import { RunBudget, RunLimitError } from "./limits.js";

const MAX_TOOL_RESULT_CHARS = 16 * 1024;
const LIVENESS_INTERVAL_MS = 2_000;
const SENSITIVE_ARGUMENT = /(?:password|passcode|passphrase|secret|senha|segredo|credential|credencial|token|\bpin\b|api[_-]?key|chave)/iu;

type Run = { controller: AbortController; sessionId: string; completedEffects: number };
type PendingTool = { runId: string; tool: string; resolve: (result: ToolResultMessage) => void };
type Observation = { tool: string; status: string; sideEffect: string; data?: JsonValue; error?: string; verification?: JsonValue };

class UnavailableTextPlanner implements TextPlanner {
  readonly id = "openai-compatible-text" as const;
  readonly model_ref = "não configurado";
  readonly context_chars = 0;
  constructor(private readonly reason: ProviderError) {}
  interpretTask(): Promise<TaskSpec> { return Promise.reject(this.reason); }
  proposeCandidates(): Promise<ActionCandidate[]> { return Promise.reject(this.reason); }
  compose(): Promise<string> { return Promise.reject(this.reason); }
  probe(): Promise<never> { return Promise.reject(this.reason); }
}

class UnavailableAgent implements AgentProvider {
  readonly id = "typesafe-jev" as const;
  readonly model_ref = "jev-latest";
  readonly context_chars = 0;
  constructor(private readonly reason: ProviderError) {}
  decide(): Promise<never> { return Promise.reject(this.reason); }
}

function unavailable(error: unknown, fallback: string): ProviderError {
  return error instanceof ProviderError ? error : new ProviderError("CONFIG_ERROR", fallback, false);
}
function configuredText(): TextPlanner {
  try { return textPlannerFromEnvironment(); }
  catch (error) { return new UnavailableTextPlanner(unavailable(error, "configuração de texto inválida")); }
}
function configuredAgent(): AgentProvider {
  try { return agentFromEnvironment(); }
  catch (error) { return new UnavailableAgent(unavailable(error, "configuração Jev inválida")); }
}

const text = configuredText();
const agent = configuredAgent();
const sessions = new Set<string>();
const runs = new Map<string, Run>();
const pendingTools = new Map<string, PendingTool>();
let initialized = false;
let closing = false;

function send(message: IpcMessage): void { if (!closing) process.stdout.write(encodeMessage(message)); }
function log(message: string): void { if (!closing) process.stderr.write(`[vox-agent-core] ${redactTextForModel(message)}\n`); }
function state(sessionId: string, runId: string, next: "thinking" | "executing" | "waiting_user" | "completed" | "failed" | "cancelled" | "cancelling"): void {
  send({ type: "state.changed", session_id: sessionId, run_id: runId, state: next });
}
function bounded(value: string, limit = MAX_TOOL_RESULT_CHARS): string {
  const redacted = redactTextForModel(value);
  return redacted.length <= limit ? redacted : `${redacted.slice(0, limit)}\n[conteúdo truncado]`;
}
function decisionState(task: TaskSpec, observations: readonly Observation[], candidates: readonly ActionCandidate[], phase: string): string {
  const safeObservations = observations.slice(-8).map((observation) => ({
    tool: observation.tool, status: observation.status, side_effect: observation.sideEffect,
    data: observation.data ? redactJsonForModel(observation.data) : undefined,
    error: observation.error ? bounded(observation.error, 512) : undefined,
    verification: observation.verification ? redactJsonForModel(observation.verification) : undefined,
  }));
  return bounded(JSON.stringify({
    task, phase, observations: safeObservations,
    candidates: candidates.map((candidate) => ({ id: candidate.id, label: candidate.label, tool: candidate.tool, effect: candidate.effect, verification: candidate.verification })),
  }), agent.context_chars || 64 * 1024);
}
function emitActivity(sessionId: string, runId: string, round: number, provider: string, model: string, messages: readonly { role: "system" | "user" | "assistant"; content: string }[]): void {
  send({
    type: "model.requested", session_id: sessionId, run_id: runId, round, provider, model_ref: model,
    messages: messages.map((message) => ({ role: message.role, content: bounded(message.content), redacted: true })), redacted: true,
  });
}
function threshold(effect: ActionCandidate["effect"]): number {
  if (effect === "read" || effect === "none") return 0.65;
  if (effect === "write") return 0.85;
  return 0.95;
}
function candidateIsSafe(candidate: ActionCandidate): boolean {
  if (candidate.effect !== effectForTool(candidate.tool)) return false;
  const rendered = JSON.stringify(candidate.arguments);
  return !SENSITIVE_ARGUMENT.test(rendered) && redactTextForModel(rendered) === rendered;
}
function candidateSignature(candidate: ActionCandidate): string { return `${candidate.tool}:${JSON.stringify(candidate.arguments)}`; }
function candidateOptions(candidates: readonly ActionCandidate[]): { id: string; description: string }[] {
  return [
    ...candidates.map((candidate) => ({ id: candidate.id, description: `${candidate.label}. Ferramenta ${candidate.tool}; efeito ${candidate.effect}; verificar: ${candidate.verification}` })),
    { id: "reobserve", description: "Observar novamente antes de qualquer efeito." },
    { id: "replan", description: "Pedir ao serviço de texto novos candidatos, sem executar nada." },
    { id: "ask_user", description: "Pedir esclarecimento porque a evidência não resolve a ambiguidade." },
    { id: "finish", description: "Encerrar somente se os resultados observados comprovarem o objetivo." },
  ];
}
function choice(answer: DecisionAnswer | undefined): { id: string; confidence: number; probabilities: Readonly<Record<string, number>> } | undefined {
  return answer?.type === "choice" ? { id: answer.choice, confidence: answer.confidence, probabilities: answer.probabilities } : undefined;
}
function awaitTool(callId: string, runId: string, tool: string, signal: AbortSignal): Promise<ToolResultMessage> {
  return new Promise((resolve, reject) => {
    const abort = () => { pendingTools.delete(callId); reject(new ProviderError("CANCELLED", "execução cancelada", false)); };
    signal.addEventListener("abort", abort, { once: true });
    pendingTools.set(callId, { runId, tool, resolve: (result) => { signal.removeEventListener("abort", abort); resolve(result); } });
  });
}
async function executeCandidate(sessionId: string, runId: string, taskGrantId: string, candidate: ActionCandidate, signal: AbortSignal): Promise<ToolResultMessage> {
  const callId = randomUUID();
  send({ type: "tool.execute.requested", session_id: sessionId, run_id: runId, task_grant_id: taskGrantId, call_id: callId, tool: candidate.tool, arguments: candidate.arguments, effect: candidate.effect });
  return awaitTool(callId, runId, candidate.tool, signal);
}
async function composeCompletion(task: TaskSpec, observations: readonly Observation[], signal: AbortSignal): Promise<string> {
  const factual = JSON.stringify({ task, observations: observations.slice(-12).map((item) => ({ tool: item.tool, status: item.status, side_effect: item.sideEffect, error: item.error, verification: item.verification, data: item.data ? redactJsonForModel(item.data) : undefined })) });
  return bounded(await text.compose({ instruction: "Escreva uma resposta final em português baseada apenas nos fatos abaixo. Não alegue sucesso para efeitos unknown, pending ou falhos. Seja breve e diga quando faltou verificação.", state: factual }, signal), 16 * 1024);
}

async function runTurn(request: Extract<IpcMessage, { type: "turn.start" }>): Promise<void> {
  if (runs.has(request.run_id)) {
    send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: "RUN_ALREADY_ACTIVE", error: "run já está ativo", retryable: false });
    return;
  }
  const controller = new AbortController();
  const run: Run = { controller, sessionId: request.session_id, completedEffects: 0 };
  runs.set(request.run_id, run);
  const budget = new RunBudget();
  const liveness = setInterval(() => send({ type: "heartbeat", request_id: `run-${request.run_id}`, runtime: process.version, healthy: true }), LIVENESS_INTERVAL_MS);
  try {
    state(request.session_id, request.run_id, "thinking");
    emitActivity(request.session_id, request.run_id, 1, text.id, text.model_ref, [
      { role: "system", content: "O serviço de texto cria somente uma especificação estruturada de tarefa." }, { role: "user", content: request.content },
    ]);
    const task = await text.interpretTask(request.content, controller.signal);
    const observations: Observation[] = [];
    const executed = new Set<string>();
    let candidates: ActionCandidate[] = [];
    let lowConfidenceReobserved = false;
    let replan = true;
    for (let round = 1; ; round += 1) {
      budget.consumeStep();
      if (replan) {
        const requestState = decisionState(task, observations, candidates, "candidate_generation");
        emitActivity(request.session_id, request.run_id, round, text.id, text.model_ref, [
          { role: "system", content: "O serviço de texto propõe candidatos válidos, sem escolher ou chamar ferramentas." }, { role: "user", content: requestState },
        ]);
        const proposed = await text.proposeCandidates({ task, state: requestState, tools: TOOL_CATALOG }, controller.signal);
        candidates = proposed.filter((candidate) => candidateIsSafe(candidate) && !executed.has(candidateSignature(candidate)));
        replan = false;
      }
      const stateForJev = decisionState(task, observations, candidates, "next_step");
      const questions = [
        { type: "choice" as const, id: "next_step", instructions: "Escolha somente o próximo passo melhor suportado pela evidência. Você não cria argumentos, não executa ferramentas e pode se abster.", options: candidateOptions(candidates) },
        { type: "score" as const, id: "evidence_quality", instructions: "Classifique a qualidade da evidência observada para avançar agora.", levels: [{ id: "low", description: "Evidência ausente ou ambígua." }, { id: "medium", description: "Há indícios, mas uma nova observação ajudaria." }, { id: "high", description: "A evidência suporta o próximo passo delimitado." }] },
        { type: "noul" as const, id: "goal_verified", statement: `O resultado esperado foi verificado por uma observação real: ${task.expected_result}` },
      ];
      const started = Date.now();
      emitActivity(request.session_id, request.run_id, round, agent.id, agent.model_ref, [{ role: "system", content: JSON.stringify({ questions }) }, { role: "user", content: stateForJev }]);
      const decision = await agent.decide({ state: stateForJev, questions }, controller.signal);
      const selected = choice(decision.answers.next_step);
      if (!selected || !candidateOptions(candidates).some((option) => option.id === selected.id)) throw new ProviderError("INVALID_RESPONSE", "Jev escolheu uma opção fora do catálogo", false);
      emitActivity(request.session_id, request.run_id, round, agent.id, decision.model_ref, [{ role: "assistant", content: JSON.stringify({ choice: selected.id, confidence: selected.confidence, probabilities: selected.probabilities, evidence_quality: decision.answers.evidence_quality, goal_verified: decision.answers.goal_verified, duration_ms: Date.now() - started }) }]);
      const candidate = candidates.find((item) => item.id === selected.id);
      if (candidate && selected.confidence < threshold(candidate.effect)) {
        if (!lowConfidenceReobserved) {
          lowConfidenceReobserved = true;
          candidates = [{ id: "safe_reobserve", tool: "desktop.list_windows", arguments: {}, label: "Listar janelas para obter nova evidência", effect: "read", verification: "A lista de janelas deve mostrar o aplicativo ou alvo relevante." }];
          continue;
        }
        candidates = []; replan = true; continue;
      }
      lowConfidenceReobserved = false;
      if (selected.id === "ask_user") {
        state(request.session_id, request.run_id, "waiting_user");
        throw new ProviderError("NEEDS_USER_INPUT", "Preciso de uma informação adicional para continuar sem supor o alvo ou o resultado.", false);
      }
      if (selected.id === "replan") { candidates = []; replan = true; continue; }
      if (selected.id === "reobserve") {
        candidates = [{ id: "reobserve_windows", tool: "desktop.list_windows", arguments: {}, label: "Reobservar janelas abertas", effect: "read", verification: "A lista de janelas atualizada deve conter evidência útil." }];
        continue;
      }
      if (selected.id === "finish") {
        const noul = decision.answers.goal_verified;
        const verified = noul?.type === "noul" && noul.noul >= 0.85 && observations.some((item) => item.verification != null && item.sideEffect !== "unknown" && item.status === "success");
        if (!verified) { candidates = []; replan = true; continue; }
        const content = await composeCompletion(task, observations, controller.signal);
        state(request.session_id, request.run_id, "completed");
        send({ type: "run.completed", session_id: request.session_id, run_id: request.run_id, seq: 1, content });
        return;
      }
      if (!candidate) throw new ProviderError("INVALID_RESPONSE", "Jev escolheu um candidato inexistente", false);
      const signature = candidateSignature(candidate);
      if (executed.has(signature)) { candidates = []; replan = true; continue; }
      executed.add(signature);
      state(request.session_id, request.run_id, "executing");
      const result = await executeCandidate(request.session_id, request.run_id, request.task_grant_id, candidate, controller.signal);
      if (result.side_effect === "applied") run.completedEffects += 1;
      const observation: Observation = { tool: candidate.tool, status: result.status, sideEffect: result.side_effect ?? "unknown", data: result.data, error: result.error, verification: result.verification };
      observations.push(observation);
      budget.observe(JSON.stringify({ tool: observation.tool, status: observation.status, data: redactJsonForModel(observation.data ?? null), verification: redactJsonForModel(observation.verification ?? null), error: observation.error }));
      send({ type: "tool.completed", session_id: request.session_id, run_id: request.run_id, call_id: result.call_id, tool: candidate.tool, status: result.status, side_effect: result.side_effect ?? "unknown", data: result.data, error: result.error, verification: result.verification });
      candidates = []; replan = true;
      state(request.session_id, request.run_id, "thinking");
    }
  } catch (error) {
    if (error instanceof RunLimitError) {
      state(request.session_id, request.run_id, "failed");
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: error.code, error: error.message, retryable: false });
    } else if (controller.signal.aborted || (error instanceof ProviderError && error.code === "CANCELLED")) {
      state(request.session_id, request.run_id, "cancelled");
      send({ type: "run.cancelled", session_id: request.session_id, run_id: request.run_id, reason: "cancelled by user", completed_effects: run.completedEffects });
    } else if (error instanceof ProviderError) {
      state(request.session_id, request.run_id, error.code === "NEEDS_USER_INPUT" ? "waiting_user" : "failed");
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: error.code, error: error.message, retryable: error.retryable });
    } else {
      state(request.session_id, request.run_id, "failed");
      send({ type: "run.failed", session_id: request.session_id, run_id: request.run_id, error_code: "CORE_ERROR", error: bounded(error instanceof Error ? error.message : "agent core failed"), retryable: false });
    }
  } finally { clearInterval(liveness); runs.delete(request.run_id); }
}

function shutdown(): void { if (!closing) { closing = true; for (const run of runs.values()) run.controller.abort(); process.stdin.destroy(); } }
async function probeAgent(requestId: string): Promise<void> {
  const controller = new AbortController();
  try {
    const result = await agent.decide({
      state: "Teste de conexão do Vox. Não há tarefa nem ação.",
      questions: [{ type: "choice", id: "probe", instructions: "Selecione a opção de conexão válida.", options: [{ id: "ok", description: "A conexão está ativa." }, { id: "retry", description: "A conexão precisa de nova tentativa." }] }],
    }, controller.signal);
    send({ type: "heartbeat", request_id: requestId, runtime: process.version, healthy: true, agent_probe: { available: true, model_ref: result.model_ref } } as IpcMessage);
  } catch (error) {
    send({ type: "heartbeat", request_id: requestId, runtime: process.version, healthy: true, agent_probe: { available: false, error: bounded(error instanceof Error ? error.message : "Jev indisponível", 512) } } as IpcMessage);
  }
}
async function probeText(requestId: string): Promise<void> {
  const controller = new AbortController();
  try {
    const result = await text.probe(controller.signal);
    send({ type: "heartbeat", request_id: requestId, runtime: process.version, healthy: true, text_probe: result } as IpcMessage);
  } catch (error) {
    send({ type: "heartbeat", request_id: requestId, runtime: process.version, healthy: true, text_probe: { available: false, model_ids: [], error: bounded(error instanceof Error ? error.message : "texto indisponível", 512) } } as IpcMessage);
  }
}
function handle(message: IpcMessage): void {
  if (closing) return;
  if (message.type === "shutdown") { shutdown(); return; }
  if (message.type === "initialize") {
    if (message.protocol !== undefined && message.protocol !== PROTOCOL_VERSION) throw new ProtocolError("VERSION_MISMATCH", "protocol version is incompatible");
    if (initialized) throw new ProtocolError("ALREADY_INITIALIZED", "core is already initialized");
    initialized = true;
    send({ type: "initialized", protocol: PROTOCOL_VERSION, runtime: process.version, build: "vox-agent-core", capabilities: ["task-spec", "candidate-planning", "jev-primary", "task-grants", agent.id, `agent_model:${agent.model_ref}`, text.id, `text_model:${text.model_ref}`] }); return;
  }
  if (!initialized) throw new ProtocolError("NOT_INITIALIZED", "initialize must succeed before other requests");
  if (message.type === "session.open") { sessions.add(message.session_id); send({ type: "session.opened", request_id: message.request_id, session_id: message.session_id }); }
  else if (message.type === "turn.start") {
    if (!sessions.has(message.session_id)) send({ type: "run.failed", session_id: message.session_id, run_id: message.run_id, error_code: "SESSION_NOT_OPEN", error: "session must be opened before a turn", retryable: false });
    else void runTurn(message);
  } else if (message.type === "turn.cancel") {
    const run = runs.get(message.run_id); run?.controller.abort(); if (run) state(run.sessionId, message.run_id, "cancelling");
  } else if (message.type === "heartbeat") {
    if ((message as unknown as { agent_probe?: boolean }).agent_probe) void probeAgent(message.request_id);
    else if ((message as unknown as { text_probe?: boolean }).text_probe) void probeText(message.request_id);
    else send({ type: "heartbeat", request_id: message.request_id, runtime: process.version, healthy: true });
  }
  else if (message.type === "tool.result") {
    const pending = pendingTools.get(message.call_id); const run = pending ? runs.get(pending.runId) : undefined;
    if (pending && run && pending.runId === message.run_id && pending.tool === message.tool && (message.session_id == null || message.session_id === run.sessionId)) { pendingTools.delete(message.call_id); pending.resolve(message); }
    else send({ type: "error", error_code: "TOOL_RESULT_MISMATCH", error: "tool result does not match a pending call", retryable: false });
  }
}
function receive(line: Buffer): void {
  try { handle(parseMessage(line.toString("utf8").replace(/\r$/u, ""))); }
  catch (error) { if (error instanceof ProtocolError) send({ type: "error", error_code: error.code, error: error.message, retryable: false }); else log(error instanceof Error ? error.message : "protocol failure"); }
}
async function readInput(): Promise<void> {
  let fragments: Buffer[] = []; let bytes = 0; let discarding = false;
  for await (const chunk of process.stdin) {
    const data = chunk as Buffer; let offset = 0;
    while (offset < data.length && !closing) {
      const newline = data.indexOf(10, offset); const end = newline < 0 ? data.length : newline; const part = data.subarray(offset, end);
      if (!discarding) { bytes += part.length; if (bytes > MAX_MESSAGE_BYTES + 1) { discarding = true; fragments = []; send({ type: "error", error_code: "MESSAGE_TOO_LARGE", error: "message exceeds 1 MiB", retryable: false }); } else fragments.push(part); }
      if (newline >= 0) { if (!discarding) receive(Buffer.concat(fragments, bytes)); fragments = []; bytes = 0; discarding = false; }
      offset = newline < 0 ? data.length : newline + 1;
    }
  }
  if (!closing && !discarding && bytes > 0) receive(Buffer.concat(fragments, bytes));
  shutdown();
}
void readInput();
