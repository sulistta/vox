# Vox

Vox é um assistente de desktop nativo: a janela é Rust/egui, o core de conversa TypeScript roda em processo privado supervisionado e os efeitos passam por um broker de política Rust. O projeto não é o CLI do Pi, não lê a configuração do Pi do usuário e não usa Tauri/Electron/WebView.

## Estado desta execução

A primeira fatia funcional está implementada e validada localmente em Linux/Wayland:

- protocolo NDJSON v1 compartilhado entre TypeScript e Rust;
- core fake textual com streaming, cancelamento e fluxo texto → tool → resultado;
- provider OpenAI-compatible textual com SSE e erros normalizados;
- broker com aprovação hash-bound, expiração e single-use;
- leitura/busca/listagem/escrita/movimentação de arquivos e argv sem shell;
- adapter xa11y Rust nativo, snapshots limitados e fixture adversarial;
- SQLite local com migração, recuperação, journal idempotente e redaction;
- janela egui com histórico, streaming, modo compacto, Parar, aprovação e preferências.

As matrizes de QA e efeitos estão em [`docs/QA.md`](docs/QA.md) e
[`docs/TOOL-MATRIX.md`](docs/TOOL-MATRIX.md). O estado de instalação, assinatura
e release está em [`docs/RELEASE.md`](docs/RELEASE.md); o processo de suporte e
vulnerabilidades está em [`docs/SUPPORT.md`](docs/SUPPORT.md).

O suporte real observado nesta máquina é Linux Ubuntu/GNOME/Wayland. Windows, macOS, X11/KDE, voz e instaladores continuam explicitamente condicionados aos testes descritos em [`docs/PLATFORM-MATRIX.md`](docs/PLATFORM-MATRIX.md) e no plano.

## Executar

```sh
pnpm install
pnpm typecheck
pnpm test
pnpm build
cargo test --workspace
cargo build -p vox-desktop
VOX_AGENT_ENTRY="$PWD/packages/agent-core/dist/main.js" ./target/debug/vox-desktop
```

O provider padrão é fake e textual. Para um endpoint OpenAI-compatible, configure `VOX_PROVIDER_BASE_URL` e `VOX_PROVIDER_MODEL`; a chave é lida apenas pelo processo e não é registrada.

Veja [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), [`docs/SECURITY.md`](docs/SECURITY.md), [`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md) e o plano em [`.plan/README.md`](.plan/README.md) antes de promover um artefato para instalação.
