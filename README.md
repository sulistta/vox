# Vox / vocs

Vox is a native Rust experiment for Wayland computer use. It keeps one XDG
RemoteDesktop + ScreenCast + PipeWire session while a task runs, asks Jev for
one bounded decision at a time, uses a configurable text provider only when
new language is needed, uses Vocaela only to ground a visual instruction, and
uses the portal for input.

## Architecture

```
Rust controller → Jev decision → policy → portal side effect
      │                │              │
      │                └ closed-set capability and arguments
      ├ artifacts ↔ TextProvider (only after RequestText or RequestPlan)
      └ fresh frame ↔ VisionProvider (only before VisualAction)
```

Rust owns the state machine and all side effects. Jev selects one capability
and closed-set arguments; it receives no pixels and writes no free text.
`OpenAiCompatibleTextProvider` writes only a requested text artifact or, after
an explicit `RequestPlan`, validated `TaskGuidance`. The
current `VocaelaVisionProvider` receives an in-memory frame plus one concise
visual instruction and returns `Found(bbox)` or `NotFound`.

The controller state contains the original goal artifact, a bounded recent
history, current artifacts, last execution result, capability availability,
desktop state, and an OS-provided application catalog. Site and application
names are runtime data; there are no application-specific skills or routes.

## Architecture invariants

1. Rust owns control flow.
2. Jev selects; it does not generate free text.
3. TextProvider generates text; it does not choose actions.
4. VisionProvider sees; it does not own high-level intent.
5. Side effects pass through PolicyEngine.
6. Invalid grounding never causes a click.
7. Application/site names are data, not code paths.
8. There is no simple/complex task classifier.
9. Screenshots stay local by default.
10. User-facing configuration happens in the UI.

## Settings and secrets

Click the Vox avatar to open Settings. It stores Jev and text-provider URLs and
model identifiers in `XDG_CONFIG_HOME/vocs/settings.json` (normally
`~/.config/vocs/settings.json`). API keys are written to the Linux Secret
Service under `vocs/typesafe_api_key` and `vocs/text_provider_api_key`; Slint
never reads a saved key back into a text field. `.env` remains a development
fallback for `TYPESAFE_API_KEY`, `TEXT_PROVIDER_API_KEY`, `VOCAELA_URL`,
`MAX_STEPS`, and `ACTION_SETTLE_MS`.

The text provider is a minimal OpenAI-compatible endpoint: it uses only
`POST {base_url}/chat/completions`, `model`, `messages`, `temperature`, and
`max_tokens`. Authentication is optional. It does not require tools, schemas,
streaming, or model enumeration.

## Development setup

```bash
sudo apt install clang libclang-dev libpipewire-0.3-dev libwayland-dev libxkbcommon-dev libdbus-1-dev libglib2.0-dev
cargo run
```

Without `--image`, Vox starts the live portal session and requests a monitor
once. A goal can be submitted through the dock or provided for development:

```bash
cargo run -- --goal "Open an application and interact with a visible item"
cargo run -- --goal "Open an application and interact with a visible item" --execute
cargo run -- --goal "Click the visible button" --image screenshot.png
```

`--execute` enables real portal input. It is deliberately rejected with
`--image`. The Vocaela `llama-server` remains an external development runtime;
future packaging belongs in `VisionRuntimeManager`, not in this controller.

## Policy and verification

Policy is Rust code and currently permits the bounded input primitives. Jev
does not supply a risk judgment, and typing or pressing a key is not treated as
an implicit commit. Confirmation will return only with an explicit, typed
`Commit` action: inferring a commit from arbitrary text or Enter would be an
application-specific heuristic.

When the next step needs open-set information absent from state, Jev can choose
`RequestPlan`. The TextProvider returns a bounded JSON `TaskGuidance`:

- an objective and up to eight constraints;
- up to sixteen typed artifacts, each either a literal or a pending text
  generation request; and
- up to sixteen unordered action hints.

Hints have no cursor, order, dependencies, or execution authority. Rust
materializes literal artifacts, offers `RequestText` only for pending generated
artifacts, and then asks Jev to select one currently resolvable hint. A hint is
consumed after one attempt. `RequestPlan` exists only without active guidance;
typed failure evidence exposes `RequestReplan` and suppresses stale hints until
replacement guidance is produced.

`RequestReplan` is available only when `replan_reason` is a typed, recorded
fact: `GroundingNotFound`, input failure, verification failure, an invalidated
step, a changed user goal, insufficient guidance, or another explicit failure.
Creating guidance clears that reason; no automatic replan or decision retry is
performed.

`PressKey`, `Hotkey`, `Scroll`, and `Wait` are never global choices. They exist
only when an active guidance hint supplies a closed, supported operand. Text is
entered only from a materialized `TextToType` artifact. A small physical-action
loop guard detects three repetitions of a one-to-four-action pattern without
new state, observation, or guidance evidence and routes to `RequestReplan`.

PipeWire remains live, but a fresh frame is acquired only for `VisualAction`
and post-input verification — the points at which pixels are actually
interpreted. The current Vocaela integration grounds a supplied visual
instruction but does not yet answer semantic questions such as which
application is open. `LaunchApp` is therefore planned only after an explicit
user request or recorded `GroundingNotFound` evidence that the current context
cannot satisfy the step.

Input delivery and semantic completion are distinct. Visual actions acquire a
fresh frame, ground it, inject input, wait, acquire another frame, and record a
cheap screen-delta result: `ScreenChanged`, `NoVisibleChange`, or `Unknown`.
A changed screen is evidence only; it is not proof that the user’s goal was
fulfilled. `Done` is offered only after a terminal hint has been delivered and
records `OperationallyCompleteUnverified`.

Only an explicit `Commit` hint is consequential. It contains a separate
preview artifact and an operation (visual action, key, or shortcut). In
`--execute` mode PolicyEngine pauses the same portal session and the dock shows
the preview with **Cancel** and **Allow once**. Navigation, typing, ordinary
Enter, scrolling, and waits remain low-risk operations; policy never guesses
that a string or application name is a commit.

The OpenAI-compatible text client reports a typed `OutputTruncated` error when
the provider returns `finish_reason: "length"`, including only safe metadata
(model, completion-token count when provided, and whether reasoning was
present). It never treats hidden reasoning as executable content. Advanced
configuration may optionally supply `max_completion_tokens` or
`reasoning_effort`; neither is required by the generic protocol.

Developer runs are written beneath `XDG_STATE_HOME/vocs/runs/<run-id>/` by
default. They include visual frames/overlays and per-step decision snapshots.
They can contain personal information and should be treated as developer data.

## Known limitations

- The current VisionProvider grounds visible targets. It does not yet answer
  arbitrary visual questions such as whether a dialog reports success. A
  separate `VisionProvider::query(frame, question)` spike is needed before
  conditional visual tasks are reliable.
- A target that is off-screen or covered may return `NotFound`. The controller
  records that result and does not click a fallback location.
- The dock may be included in a captured monitor. This must be benchmarked with
  a saved debug frame before introducing a hide/capture/restore guard.
- `TypeArtifact` uses the embedded Wayland clipboard owner followed by portal
  `Ctrl+V`; it is not offered if that backend cannot initialize. This preserves
  Unicode text without a character-by-character fallback. It still needs live
  compositor coverage before being considered portable across all Wayland
  environments.
- Vocaela is visual-only in this project. The referenced checkpoint is
  CC BY-NC-SA; it is suitable for this prototype but must remain replaceable.
- Voice, OCR, AT-SPI, DOM automation, EIS/libei, and browser-specific adapters
  are intentionally outside this refactor.

## Manual test guide

1. Open Settings and save Jev credentials; test the connection.
2. Optionally configure and test a text provider.
3. Run dry mode and inspect a developer run.
4. Authorize the portal once and confirm a PipeWire frame arrives.
5. Test an OS-discovered application launch.
6. Test a visible grounding action in dry mode, then with `--execute`.
7. Test `RequestText`, `RequestPlan`, `AskUser`, `ReplyToUser`, `Scroll`, `PressKey`,
   and cancellation independently.

Run the offline checks with:

```bash
cargo fmt --check
cargo check
cargo test
```
