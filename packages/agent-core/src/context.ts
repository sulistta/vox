import type { ProviderMessage } from "@vox/provider-adapters";
import { RunLimitError } from "./limits.js";

export interface ConversationEntry {
  message: ProviderMessage;
  required: boolean;
}

function messageCost(message: ProviderMessage): number {
  return message.role.length + message.content.length + 8;
}

/**
 * Preserve the current task, system constraints and uncertain observations,
 * then fill the remaining model context with the newest history. The original
 * entries are retained so subsequent rounds can compact against the same
 * complete turn state instead of silently mutating it.
 */
export function compactConversation(entries: readonly ConversationEntry[], maxCharacters: number): ProviderMessage[] {
  if (!Number.isSafeInteger(maxCharacters) || maxCharacters < 256) {
    throw new RunLimitError("CONTEXT_LIMIT", "context budget is too small");
  }
  const selected = new Set<number>();
  let characters = 0;
  for (const [index, entry] of entries.entries()) {
    if (!entry.required) continue;
    const next = characters + messageCost(entry.message);
    if (next > maxCharacters || selected.size >= 128) {
      throw new RunLimitError("CONTEXT_LIMIT", "required context does not fit the model budget");
    }
    selected.add(index);
    characters = next;
  }
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    if (selected.has(index) || selected.size >= 128) continue;
    const entry = entries[index];
    if (!entry) continue;
    const next = characters + messageCost(entry.message);
    if (next > maxCharacters) continue;
    selected.add(index);
    characters = next;
  }
  const messages = [...selected]
    .sort((left, right) => left - right)
    .map((index) => entries[index]?.message)
    .filter((message): message is ProviderMessage => message !== undefined);
  const dropped = entries.length - selected.size;
  const marker: ProviderMessage = {
    role: "system",
    content: `[${dropped} mensagens antigas foram omitidas por limite de contexto; não suponha que elas ainda estejam disponíveis.]`,
  };
  if (dropped > 0 && messages.length < 128 && characters + messageCost(marker) <= maxCharacters) {
    messages.splice(1, 0, marker);
  }
  return messages;
}

