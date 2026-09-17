# Vox

Vox é um assistente de desktop nativo: a janela é Rust/egui, o core de conversa TypeScript roda em processo privado supervisionado e os efeitos passam por um broker de política Rust. O projeto não é o CLI do Pi, não lê a configuração do Pi do usuário e não usa Tauri/Electron/WebView.

## Estado desta execução

A primeira fatia funcional está implementada e validada localmente em Linux/Wayland:

- protocolo NDJSON v1 compartilhado entre TypeScript e Rust;
- loop estruturado de modelo com streaming, cancelamento, histórico, tool → resultado → próxima rodada e atividade redigida;
- provider OpenAI-compatible textual com SSE e erros normalizados; smoke remoto controlado, sem tools;
- broker com aprovação hash-bound, expiração e single-use;
- leitura/busca/listagem/escrita/movimentação de arquivos em pastas explicitamente permitidas e argv sem shell;
- adapter xa11y Rust nativo, snapshots limitados e fixture adversarial;
- SQLite local v4 com migração, recuperação, journal idempotente, rascunhos por sessão e redação;
- janela egui com histórico, streaming, modo compacto, Parar, aprovação e preferências.

As matrizes de QA e efeitos estão em [`docs/QA.md`](docs/QA.md) e
[`docs/TOOL-MATRIX.md`](docs/TOOL-MATRIX.md). O estado de instalação, assinatura
e release está em [`docs/RELEASE.md`](docs/RELEASE.md); o processo de suporte e
vulnerabilidades está em [`docs/SUPPORT.md`](docs/SUPPORT.md).

O suporte real observado nesta máquina é Linux Ubuntu/GNOME/Wayland. Windows, macOS, X11/KDE, voz e instaladores continuam explicitamente condicionados aos testes descritos em [`docs/PLATFORM-MATRIX.md`](docs/PLATFORM-MATRIX.md) e no plano.

A auditoria atual em [`.plan/PRONTIDAO-2026-09-17.md`](.plan/PRONTIDAO-2026-09-17.md)
classifica o checkout como preview de desenvolvimento integrado, não como
release ou beta universal.

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

O provider padrão é uma demonstração textual segura e **não executa ferramentas**. Para operar o computador, configure um endpoint OpenAI-compatible e um modelo em Preferências, ou inicie pelo ambiente com `VOX_PROVIDER=openai-compatible`, `VOX_PROVIDER_BASE_URL` e `VOX_PROVIDER_MODEL`. A chave é lida apenas pelo processo privado e não é registrada. Em Preferências, escolha também as pastas que o Vox pode acessar; o padrão é Desktop, Documentos, Downloads e, no desenvolvimento, o checkout atual — nunca a pasta pessoal inteira.

Veja [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), [`docs/SECURITY.md`](docs/SECURITY.md), [`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md) e o plano em [`.plan/README.md`](.plan/README.md) antes de promover um artefato para instalação.
