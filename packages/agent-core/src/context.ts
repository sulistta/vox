import type { ContextMessage } from "@vox/protocol";

export class ContextCompactionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ContextCompactionError";
  }
}

export interface CompactedContext {
  messages: ContextMessage[];
  dropped: number;
  characters: number;
}

const requiredMessage = (message: ContextMessage, index: number): boolean =>
  index === 0 || message.role === "system" || message.effect === "pending" || message.effect === "unknown";

const messageCost = (message: ContextMessage): number => message.role.length + message.content.length + 4;

/**
 * Keep objective/constraints, all uncertain effects, and the newest context
 * that fits. Dropped context is represented explicitly; it is never silently
 * presented as if the model had seen the complete history.
 */
export function compactContext(messages: readonly ContextMessage[], maxCharacters: number): CompactedContext {
  if (!Number.isSafeInteger(maxCharacters) || maxCharacters < 256) {
    throw new ContextCompactionError("context budget is too small");
  }
  const indexed = messages.map((message, index) => ({ message, index }));
  const selected = new Set<number>();
  for (const item of indexed) if (requiredMessage(item.message, item.index)) selected.add(item.index);
  let characters = 0;
  for (const index of selected) {
    const message = messages[index];
    if (!message) throw new ContextCompactionError("context index is invalid");
    characters += messageCost(message);
  }
  if (characters > maxCharacters) throw new ContextCompactionError("required context does not fit the model budget");

  for (let index = messages.length - 1; index >= 0; index -= 1) {
    if (selected.has(index)) continue;
    const message = messages[index];
    if (!message) throw new ContextCompactionError("context index is invalid");
    const next = characters + messageCost(message);
    if (next > maxCharacters) continue;
    selected.add(index);
    characters = next;
  }
  const ordered: ContextMessage[] = [];
  for (const index of [...selected].sort((left, right) => left - right)) {
    const message = messages[index];
    if (!message) throw new ContextCompactionError("context index is invalid");
    ordered.push(message);
  }
  const dropped = messages.length - ordered.length;
  if (dropped > 0) {
    const marker: ContextMessage = {
      role: "system",
      content: `[contexto compactado: ${dropped} mensagens antigas não foram incluídas; confirme novamente qualquer fato necessário]`,
    };
    if (characters + messageCost(marker) <= maxCharacters) {
      const firstNonSystem = ordered.findIndex((message) => message.role !== "system");
      const insertion = firstNonSystem < 0 ? ordered.length : firstNonSystem;
      ordered.splice(insertion, 0, marker);
      characters += messageCost(marker);
    }
  }
  return { messages: ordered, dropped, characters };
}

export function formatContext(messages: readonly ContextMessage[], currentPrompt: string): string {
  const history = messages.map((message) => `[${message.role}] ${message.content}`).join("\n");
  return history ? `${history}\n[user] ${currentPrompt}` : currentPrompt;
}
