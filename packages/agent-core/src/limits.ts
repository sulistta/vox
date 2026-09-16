export class RunLimitError extends Error {
  constructor(readonly code: "CONTEXT_LIMIT" | "STEP_LIMIT" | "STAGNATION_LIMIT" | "RUN_TIMEOUT", message: string) {
    super(message);
    this.name = "RunLimitError";
  }
}

export interface RunBudgetOptions {
  maxPromptChars?: number;
  maxOutputChars?: number;
  maxSteps?: number;
  maxDurationMs?: number;
  stagnationLimit?: number;
}

/**
 * A small, deterministic guard around a turn. It does not retry effects and
 * it treats identical observations as stagnation rather than progress.
 */
export class RunBudget {
  readonly maxPromptChars: number;
  readonly maxOutputChars: number;
  readonly maxSteps: number;
  readonly maxDurationMs: number;
  readonly stagnationLimit: number;
  private readonly startedAt = Date.now();
  private steps = 0;
  private lastObservation: string | undefined;
  private repeatedObservations = 0;

  constructor(options: RunBudgetOptions = {}) {
    this.maxPromptChars = options.maxPromptChars ?? 64 * 1024;
    this.maxOutputChars = options.maxOutputChars ?? 256 * 1024;
    this.maxSteps = options.maxSteps ?? 30;
    this.maxDurationMs = options.maxDurationMs ?? 10 * 60 * 1000;
    this.stagnationLimit = options.stagnationLimit ?? 3;
  }

  validatePrompt(prompt: string): void {
    if (prompt.length > this.maxPromptChars) {
      throw new RunLimitError("CONTEXT_LIMIT", `prompt exceeds ${this.maxPromptChars} characters`);
    }
  }

  validateOutput(output: string): void {
    if (output.length > this.maxOutputChars) {
      throw new RunLimitError("CONTEXT_LIMIT", "output exceeds " + this.maxOutputChars + " characters");
    }
  }

  consumeStep(): void {
    this.checkTime();
    if (this.steps >= this.maxSteps) {
      throw new RunLimitError("STEP_LIMIT", `run exceeded the ${this.maxSteps}-step budget`);
    }
    this.steps += 1;
  }

  observe(fingerprint: string): void {
    this.checkTime();
    if (this.lastObservation === fingerprint) {
      this.repeatedObservations += 1;
    } else {
      this.lastObservation = fingerprint;
      this.repeatedObservations = 1;
    }
    if (this.repeatedObservations >= this.stagnationLimit) {
      throw new RunLimitError(
        "STAGNATION_LIMIT",
        `run made no observable progress for ${this.stagnationLimit} cycles`,
      );
    }
  }

  checkTime(): void {
    if (Date.now() - this.startedAt >= this.maxDurationMs) {
      throw new RunLimitError("RUN_TIMEOUT", `run exceeded the ${this.maxDurationMs}ms time budget`);
    }
  }

  get usedSteps(): number {
    return this.steps;
  }
}
