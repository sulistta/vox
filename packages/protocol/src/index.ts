import { ipcSchema } from "./ipc-schema.js";

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

export type ContextRole = "system" | "user" | "assistant" | "tool";
export type ContextEffect = "none" | "pending" | "applied" | "unknown";

export interface ContextMessage {
  role: ContextRole;
  content: string;
  effect?: ContextEffect;
}

export interface TurnStartRequest {
  type: "turn.start";
  request_id: string;
  session_id: string;
  run_id: string;
  content: string;
  source?: "text" | "voice";
  model_ref?: string;
  context?: ContextMessage[];
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

/**
 * A bounded, redacted copy of one chat message that was sent to a provider.
 * It exists solely for the user's transparency view; it is never a model
 * response, a tool authorization, or a substitute for the provider payload.
 */
export interface ModelActivityMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface ModelRequestedEvent {
  type: "model.requested";
  session_id: string;
  run_id: string;
  round: number;
  provider: string;
  model_ref?: string;
  messages: ModelActivityMessage[];
  /** The core promises these messages have passed its secret redaction filter. */
  redacted: true;
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
  | ModelRequestedEvent
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

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

type Schema = {
  type?: string | string[]; const?: unknown; enum?: unknown[]; minLength?: number; maxLength?: number;
  "x-maxUtf8Bytes"?: number; minimum?: number; maximum?: number;
  required?: string[]; properties?: Record<string, Schema>; items?: Schema; minItems?: number; maxItems?: number;
};
function validateSchema(value: unknown, schema: Schema, field = "message"): void {
  const invalid = (): never => { throw new ProtocolError("INVALID_FIELD", `${field} is invalid`); };
  if ("const" in schema && value !== schema.const) invalid();
  if (schema.enum && !schema.enum.includes(value)) invalid();
  const kind = Array.isArray(schema.type) ? schema.type.find((type) => type !== "null") : schema.type;
  if (value === null && Array.isArray(schema.type) && schema.type.includes("null")) return;
  switch (kind) {
    case "string":
      if (typeof value !== "string" || [...value].length < (schema.minLength ?? 0)
          || Buffer.byteLength(value, "utf8") > (schema["x-maxUtf8Bytes"] ?? Infinity)) invalid();
      break;
    case "object": {
      if (!isObject(value)) return invalid();
      for (const key of schema.required ?? []) if (!(key in value)) invalid();
      for (const [key, rule] of Object.entries(schema.properties ?? {})) {
        if (Object.hasOwn(value, key)) validateSchema(value[key], rule, key);
      }
      break;
    }
    case "array":
      if (!Array.isArray(value)) return invalid();
      if (value.length < (schema.minItems ?? 0) || value.length > (schema.maxItems ?? Infinity)) invalid();
      if (schema.items) for (const item of value) validateSchema(item, schema.items, field);
      break;
    case "boolean": if (typeof value !== "boolean") invalid(); break;
    case "number": case "integer":
      if (typeof value !== "number" || !Number.isFinite(value) || (schema.type === "integer" && !Number.isInteger(value))
          || value < (schema.minimum ?? -Infinity) || value > (schema.maximum ?? Infinity)) invalid();
      break;
    case undefined: break;
    default: throw new Error("unsupported IPC schema type");
  }
}

// JSON.parse accepts lone UTF-16 surrogates and overflowing numbers; serde_json does not.
function validateJson(value: unknown, depth = 0): void {
  if (depth >= 128 && typeof value === "object" && value !== null) throw new ProtocolError("INVALID_JSON", "JSON nesting limit exceeded");
  if (typeof value === "number" && !Number.isFinite(value)) throw new ProtocolError("INVALID_JSON", "non-finite JSON number");
  if (typeof value === "string" && /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value)) {
    throw new ProtocolError("INVALID_JSON", "unpaired Unicode surrogate");
  }
  if (Array.isArray(value)) for (const item of value) validateJson(item, depth + 1);
  else if (isObject(value)) for (const [key, item] of Object.entries(value)) { validateJson(key); validateJson(item, depth + 1); }
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
  validateJson(value);
  const kind = value.type;
  const shapes: Record<string, Schema> = ipcSchema.$defs;
  if (typeof kind !== "string" || !Object.hasOwn(shapes, kind)) throw new ProtocolError("UNKNOWN_TYPE", "unknown IPC message type");
  if ((kind === "initialize" || kind === "initialized") && value.protocol !== undefined && value.protocol !== PROTOCOL_VERSION) {
    throw new ProtocolError("VERSION_MISMATCH", "protocol version is incompatible");
  }
  validateSchema(value, shapes[kind]!);
  return value as unknown as IpcMessage;
}

export function isTerminalState(state: RunState): boolean {
  return state === "completed" || state === "failed" || state === "cancelled";
}
