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
      ├ artifacts ↔ TextProvider (only after RequestText)
      └ fresh frame ↔ VisionProvider (only before VisualAction)
```

Rust owns the state machine and all side effects. Jev selects one capability
and closed-set arguments; it receives no pixels and writes no free text.
`OpenAiCompatibleTextProvider` writes only a requested text artifact. The
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
`--image`. High and medium risk actions require an explicit “Allow once” in
the dock. The Vocaela `llama-server` remains an external development runtime;
future packaging belongs in `VisionRuntimeManager`, not in this controller.

## Policy and verification

Policy is Rust code. It takes the higher of the action baseline and Jev’s risk
signal. It gates using the selected action's probability and its margin over the
runner-up, not Jev's derived `confidence`: the latter changes meaning when the
dynamic Choice set changes size. Defaults are 0.50 / 0.40 / 0.60 selected
probability for low / medium / high risk, with a 0.04 minimum margin. Medium and
high risk still always request confirmation. These are calibration inputs, not
claims of semantic correctness.

Input delivery and semantic completion are distinct. Visual actions acquire a
fresh frame, ground it, inject input, wait, acquire another frame, and record a
cheap screen-delta result: `ScreenChanged`, `NoVisibleChange`, or `Unknown`.
A changed screen is evidence only; it is not proof that the user’s goal was
fulfilled.

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
- The portal text backend currently uses supported keysyms. Clipboard-based
  Unicode paste is isolated as the next input-backend step; arbitrary Unicode
  text is not production-ready until that portal path is validated.
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
6. Test a visible grounding action in dry mode, then with an explicit approval.
7. Test `RequestText`, `AskUser`, `ReplyToUser`, `Scroll`, `PressKey`, `Wait`,
   and cancellation independently.

Run the offline checks with:

```bash
cargo fmt --check
cargo check
cargo test
```
