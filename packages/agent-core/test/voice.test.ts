import test from "node:test";
import assert from "node:assert/strict";
import { UnavailableTranscriber, VoiceController, type Transcriber } from "../src/voice.js";

const transcriber: Transcriber = {
  async transcribe(audio) {
    return audio.byteLength ? "  transcrição editável  " : "";
  },
};

test("voice capture requires explicit review before accepting a turn", async () => {
  let now = 1_000;
  const voice = new VoiceController(transcriber, { now: () => now, maxDurationMs: 10_000 });
  assert.equal(voice.start().state, "recording");
  now += 125;
  const review = await voice.stop(new Uint8Array([1, 2]));
  assert.equal(review.state, "review");
  assert.equal(review.transcript, "transcrição editável");
  voice.edit("texto revisado");
  assert.deepEqual(voice.acceptTurn(), { content: "texto revisado", source: "voice" });
  assert.equal(voice.snapshot().state, "idle");
});

test("empty audio and transcription failure never create a turn", async () => {
  const empty = new VoiceController(transcriber);
  empty.start();
  assert.equal((await empty.stop(new Uint8Array())).errorCode, "EMPTY_AUDIO");
  const failing = new VoiceController({
    async transcribe() {
      throw new Error("permission denied by microphone");
    },
  });
  failing.start();
  const result = await failing.stop(new Uint8Array([1]));
  assert.equal(result.errorCode, "PERMISSION_DENIED");
  assert.equal(failing.accept(), undefined);
});

test("an unavailable transcription engine cannot create a voice turn", async () => {
  const voice = new VoiceController(new UnavailableTranscriber("STT local não está configurado"));
  voice.start();
  const result = await voice.stop(new Uint8Array([1, 2]));
  assert.equal(result.state, "failed");
  assert.equal(result.errorCode, "TRANSCRIPTION_UNAVAILABLE");
  assert.equal(result.error, "STT local não está configurado");
  assert.equal(voice.acceptTurn(), undefined);
});

test("cancelled capture is terminal and does not leak a transcript", () => {
  const voice = new VoiceController(transcriber);
  voice.start();
  assert.equal(voice.cancel().state, "cancelled");
  assert.equal(voice.snapshot().transcript, "");
  assert.equal(voice.accept(), undefined);
});

test("a late transcription cannot reopen review after cancellation", async () => {
  let resolveTranscription!: (value: string) => void;
  const voice = new VoiceController({
    transcribe: () => new Promise<string>((resolve) => {
      resolveTranscription = resolve;
    }),
  });
  voice.start();
  const pending = voice.stop(new Uint8Array([1]));
  assert.equal(voice.snapshot().state, "transcribing");
  assert.equal(voice.cancel().state, "cancelled");
  resolveTranscription("não deve aparecer");
  await pending;
  assert.equal(voice.snapshot().state, "cancelled");
  assert.equal(voice.snapshot().transcript, "");
});

test("recording timeout fails without continuous capture", () => {
  let callback: (() => void) | undefined;
  const voice = new VoiceController(transcriber, {
    maxDurationMs: 1_000,
    timer: (next) => {
      callback = next;
      const handle = setTimeout(() => undefined, 60_000);
      handle.unref();
      return handle;
    },
    clearTimer: (handle) => clearTimeout(handle),
  });
  voice.start();
  callback?.();
  assert.equal(voice.snapshot().errorCode, "MAX_DURATION");
});
