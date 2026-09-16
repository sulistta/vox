export type VoiceState = "idle" | "recording" | "transcribing" | "review" | "failed" | "cancelled";

export type VoiceErrorCode =
  | "ALREADY_RECORDING"
  | "NOT_RECORDING"
  | "EMPTY_AUDIO"
  | "EMPTY_TRANSCRIPT"
  | "MAX_DURATION"
  | "PERMISSION_DENIED"
  | "TRANSCRIPTION_FAILED";

export interface Transcriber {
  transcribe(audio: Uint8Array, signal: AbortSignal): Promise<string>;
}

export interface VoiceSnapshot {
  state: VoiceState;
  transcript: string;
  errorCode?: VoiceErrorCode;
  error?: string;
  elapsedMs: number;
}

export interface AcceptedVoiceTurn {
  content: string;
  source: "voice";
}

export interface VoiceControllerOptions {
  maxDurationMs?: number;
  now?: () => number;
  timer?: (callback: () => void, delayMs: number) => ReturnType<typeof setTimeout>;
  clearTimer?: (handle: ReturnType<typeof setTimeout>) => void;
}

/**
 * Platform-neutral voice contract. Native microphone capture owns the bytes;
 * this controller owns lifecycle, cancellation and the explicit review gate
 * before a transcript becomes a turn.
 */
export class VoiceController {
  private current: VoiceSnapshot = { state: "idle", transcript: "", elapsedMs: 0 };
  private startedAt = 0;
  private timerHandle: ReturnType<typeof setTimeout> | undefined;
  private readonly maxDurationMs: number;
  private readonly now: () => number;
  private readonly timer: NonNullable<VoiceControllerOptions["timer"]>;
  private readonly clearTimer: NonNullable<VoiceControllerOptions["clearTimer"]>;
  private operation = 0;

  constructor(
    private readonly transcriber: Transcriber,
    options: VoiceControllerOptions = {},
  ) {
    this.maxDurationMs = Math.max(1_000, options.maxDurationMs ?? 60_000);
    this.now = options.now ?? Date.now;
    this.timer = options.timer ?? ((callback, delayMs) => setTimeout(callback, delayMs));
    this.clearTimer = options.clearTimer ?? ((handle) => clearTimeout(handle));
  }

  snapshot(): VoiceSnapshot {
    return { ...this.current };
  }

  start(): VoiceSnapshot {
    if (this.current.state === "recording" || this.current.state === "transcribing") {
      return this.fail("ALREADY_RECORDING", "a voice capture is already active");
    }
    this.clearScheduledStop();
    const operation = ++this.operation;
    this.startedAt = this.now();
    this.current = { state: "recording", transcript: "", elapsedMs: 0 };
    this.timerHandle = this.timer(() => {
      if (this.current.state === "recording" && operation === this.operation) {
        this.current = {
          state: "failed",
          transcript: "",
          errorCode: "MAX_DURATION",
          error: "voice capture reached its maximum duration",
          elapsedMs: this.maxDurationMs,
        };
      }
    }, this.maxDurationMs);
    return this.snapshot();
  }

  async stop(audio: Uint8Array, signal: AbortSignal = new AbortController().signal): Promise<VoiceSnapshot> {
    if (this.current.state !== "recording") return this.fail("NOT_RECORDING", "voice capture is not active");
    this.clearScheduledStop();
    const elapsedMs = Math.min(this.maxDurationMs, Math.max(0, this.now() - this.startedAt));
    if (audio.byteLength === 0) return this.fail("EMPTY_AUDIO", "no audio was captured", elapsedMs);
    this.current = { state: "transcribing", transcript: "", elapsedMs };
    const operation = this.operation;
    try {
      const transcript = (await this.transcriber.transcribe(audio, signal)).trim();
      if (signal.aborted || operation !== this.operation) return this.snapshot();
      if (!transcript) return this.fail("EMPTY_TRANSCRIPT", "transcription returned no text", elapsedMs);
      this.current = { state: "review", transcript, elapsedMs };
    } catch (error) {
      if (operation !== this.operation) return this.snapshot();
      if (signal.aborted) return this.cancel(elapsedMs);
      const code = error instanceof Error && /permission/iu.test(error.message)
        ? "PERMISSION_DENIED"
        : "TRANSCRIPTION_FAILED";
      return this.fail(code, error instanceof Error ? error.message : "transcription failed", elapsedMs);
    }
    return this.snapshot();
  }

  edit(transcript: string): VoiceSnapshot {
    if (this.current.state !== "review") return this.fail("NOT_RECORDING", "there is no transcript under review");
    this.current = { ...this.current, transcript };
    return this.snapshot();
  }

  accept(): string | undefined {
    if (this.current.state !== "review" || !this.current.transcript.trim()) return undefined;
    const transcript = this.current.transcript.trim();
    this.current = { state: "idle", transcript: "", elapsedMs: 0 };
    return transcript;
  }

  acceptTurn(): AcceptedVoiceTurn | undefined {
    const content = this.accept();
    return content === undefined ? undefined : { content, source: "voice" };
  }

  cancel(elapsedMs = this.current.elapsedMs): VoiceSnapshot {
    this.clearScheduledStop();
    this.operation += 1;
    this.current = { state: "cancelled", transcript: "", elapsedMs };
    return this.snapshot();
  }

  private fail(code: VoiceErrorCode, error: string, elapsedMs = this.current.elapsedMs): VoiceSnapshot {
    this.clearScheduledStop();
    this.operation += 1;
    this.current = { state: "failed", transcript: "", errorCode: code, error, elapsedMs };
    return this.snapshot();
  }

  private clearScheduledStop(): void {
    if (this.timerHandle !== undefined) this.clearTimer(this.timerHandle);
    this.timerHandle = undefined;
  }
}
