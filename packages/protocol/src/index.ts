export const PROTOCOL_VERSION = 1 as const;
export const MAX_MESSAGE_BYTES = 1024 * 1024;

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonObject | JsonValue[];
export type JsonObject = { [key: string]: JsonValue };

export type RunState =
  | "idle"
  | "receiving"
  | "transcribing"
  | "thinking"
  | "executing"
  | "waiting_user"
  | "awaiting_approval"
  | "cancelling"
  | "completed"
  | "failed"
  | "cancelled";

export type EffectClass = "none" | "read" | "write" | "external" | "arbitrary";
export type ToolStatus = "success" | "error" | "cancelled" | "unknown";
export type SideEffect = "none" | "applied" | "unknown";

export interface InitializeRequest {
  type: "initialize";
  request_id?: string;
  protocol?: number;
  build?: string;
}

export interface SessionOpenRequest {
  type: "session.open";
  request_id: string;
  session_id: string;
}

export interface TurnStartRequest {
  type: "turn.start";
  request_id: string;
  session_id: string;
  run_id: string;
  content: string;
  source?: "text" | "voice";
  model_ref?: string;
}

export interface TurnCancelRequest {
  type: "turn.cancel";
  request_id: string;
  run_id: string;
}

export interface ToolResultMessage {
  type: "tool.result";
  request_id?: string;
  session_id?: string;
  run_id: string;
  call_id: string;
  tool: string;
  status: ToolStatus;
  data?: JsonValue;
  error_code?: string;
  error?: string;
  retryable?: boolean;
  side_effect?: SideEffect;
  duration_ms?: number;
  truncated?: boolean;
  verification?: JsonValue;
}

export interface ShutdownRequest {
  type: "shutdown";
  request_id?: string;
}

export interface HeartbeatRequest {
  type: "heartbeat";
  request_id: string;
}

export type IpcRequest =
  | InitializeRequest
  | SessionOpenRequest
  | TurnStartRequest
  | TurnCancelRequest
  | ToolResultMessage
  | HeartbeatRequest
  | ShutdownRequest;

export interface InitializedEvent {
  type: "initialized";
  protocol: 1;
  runtime: string;
  build: string;
  capabilities: string[];
}

export interface SessionOpenedEvent {
  type: "session.opened";
  request_id: string;
  session_id: string;
}

export interface StateChangedEvent {
  type: "state.changed";
  session_id: string;
  run_id: string;
  state: RunState;
}

export interface MessageDeltaEvent {
  type: "message.delta";
  session_id: string;
  run_id: string;
  seq: number;
  delta: string;
}

interface ToolEventFields {
  session_id: string;
  run_id: string;
  call_id: string;
  tool: string;
  arguments: JsonObject;
  effect: EffectClass;
}

export interface ToolStartedEvent extends ToolEventFields {
  type: "tool.started";
}

export interface ToolRequestedEvent extends ToolEventFields {
  type: "tool.execute.requested";
}

export interface ToolCompletedEvent {
  type: "tool.completed";
  session_id: string;
  run_id: string;
  call_id: string;
  tool: string;
  status: ToolStatus;
  side_effect: SideEffect;
  data?: JsonValue;
  error?: string;
  verification?: JsonValue;
}

export interface ApprovalRequiredEvent {
  type: "approval.required";
  session_id: string;
  run_id: string;
  call_id: string;
  tool: string;
  arguments: JsonObject;
  effect: EffectClass;
  approval_id: string;
  expires_at: string;
}

export interface RunCompletedEvent {
  type: "run.completed";
  session_id: string;
  run_id: string;
  seq: number;
  content: string;
}

export interface RunFailedEvent {
  type: "run.failed";
  session_id: string;
  run_id: string;
  error_code: string;
  error: string;
  retryable: boolean;
}

export interface RunCancelledEvent {
  type: "run.cancelled";
  session_id: string;
  run_id: string;
  reason: string;
  completed_effects: number;
}

export interface ErrorEvent {
  type: "error";
  request_id?: string;
  error_code: string;
  error: string;
  retryable: boolean;
}

export interface HeartbeatEvent {
  type: "heartbeat";
  request_id: string;
  runtime: string;
  healthy: true;
}

export type IpcEvent =
  | InitializedEvent
  | SessionOpenedEvent
  | StateChangedEvent
  | MessageDeltaEvent
  | ToolStartedEvent
  | ToolRequestedEvent
  | ToolCompletedEvent
  | ApprovalRequiredEvent
  | RunCompletedEvent
  | RunFailedEvent
  | RunCancelledEvent
  | HeartbeatEvent
  | ErrorEvent;

export type IpcMessage = IpcRequest | IpcEvent;

const allowedTypes = new Set([
  "initialize",
  "initialized",
  "session.open",
  "session.opened",
  "turn.start",
  "turn.cancel",
  "tool.result",
  "tool.started",
  "tool.execute.requested",
  "tool.completed",
  "approval.required",
  "state.changed",
  "message.delta",
  "run.completed",
  "run.failed",
  "run.cancelled",
  "heartbeat",
  "shutdown",
  "error",
]);

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requiredString(object: Record<string, unknown>, key: string): string {
  const value = object[key];
  if (typeof value !== "string" || value.length === 0 || value.length > 128) {
    throw new ProtocolError("INVALID_FIELD", `${key} must be a non-empty string up to 128 characters`);
  }
  return value;
}

export class ProtocolError extends Error {
  constructor(readonly code: string, message: string) {
    super(message);
    this.name = "ProtocolError";
  }
}

export function encodeMessage(message: IpcMessage): string {
  const json = JSON.stringify(message);
  const bytes = Buffer.byteLength(json, "utf8");
  if (bytes > MAX_MESSAGE_BYTES) throw new ProtocolError("MESSAGE_TOO_LARGE", `message is ${bytes} bytes`);
  return `${json}\n`;
}

export function parseMessage(line: string): IpcMessage {
  if (Buffer.byteLength(line, "utf8") > MAX_MESSAGE_BYTES) throw new ProtocolError("MESSAGE_TOO_LARGE", "message exceeds 1 MiB");
  let value: unknown;
  try {
    value = JSON.parse(line);
  } catch {
    throw new ProtocolError("INVALID_JSON", "message is not valid JSON");
  }
  if (!isObject(value)) throw new ProtocolError("INVALID_MESSAGE", "message must be a JSON object");
  const kind = value.type;
  if (typeof kind !== "string" || !allowedTypes.has(kind)) throw new ProtocolError("UNKNOWN_TYPE", "unknown IPC message type");
  if (kind === "turn.start") {
    requiredString(value, "request_id");
    requiredString(value, "session_id");
    requiredString(value, "run_id");
    if (typeof value.content !== "string" || value.content.length > 1_048_576) throw new ProtocolError("INVALID_FIELD", "content is invalid");
    if (value.source !== undefined && value.source !== "text" && value.source !== "voice") throw new ProtocolError("INVALID_FIELD", "source is invalid");
    if (value.model_ref !== undefined) requiredString(value, "model_ref");
  }
  if (kind === "turn.cancel") {
    requiredString(value, "request_id");
    requiredString(value, "run_id");
  }
  if (kind === "heartbeat") requiredString(value, "request_id");
  if (kind === "tool.result") {
    requiredString(value, "run_id");
    requiredString(value, "call_id");
    requiredString(value, "tool");
    if (!["success", "error", "cancelled", "unknown"].includes(String(value.status))) throw new ProtocolError("INVALID_FIELD", "invalid tool status");
  }
  if (kind === "initialized" && value.protocol !== PROTOCOL_VERSION) throw new ProtocolError("VERSION_MISMATCH", "protocol version is incompatible");
  return value as unknown as IpcMessage;
}

export function isTerminalState(state: RunState): boolean {
  return state === "completed" || state === "failed" || state === "cancelled";
}
