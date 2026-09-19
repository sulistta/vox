# Vox

Vox é um assistente de desktop nativo: a janela é Rust/egui, o core de conversa TypeScript roda em processo privado supervisionado e os efeitos passam por um broker de política Rust. O projeto não é o CLI do Pi, não lê a configuração do Pi do usuário e não usa Tauri/Electron/WebView.

## Estado desta execução

A primeira fatia funcional está implementada e validada localmente em Linux/Wayland:

- protocolo NDJSON v1 compartilhado entre TypeScript e Rust;
- Jev como agente principal, com perguntas tipadas, confiança e atividade redigida;
- provider OpenAI-compatible somente para `TaskSpec`, candidatos e conteúdo livre;
- broker com `TaskGrant` por run, journal idempotente e bloqueio de efeitos incertos;
- leitura/busca/listagem/escrita/movimentação de arquivos em pastas explicitamente permitidas e argv sem shell;
- adapter xa11y Rust nativo, snapshots limitados e fixture adversarial;
- SQLite local v6 com migração, recuperação, grants, rascunhos por sessão e redação;
- janela egui com histórico, atividade, modo compacto, Parar e preferências.

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

Para iniciar uma tarefa, configure os dois serviços em Preferências: **Agente Jev** e **Texto e planejamento**. Pelo ambiente, use `VOX_AGENT_API_KEY`, `VOX_AGENT_MODEL`, `VOX_TEXT_BASE_URL`, `VOX_TEXT_MODEL` e `VOX_TEXT_API_KEY`. Jev escolhe cada próximo passo entre candidatos delimitados; o serviço de texto só cria a especificação, candidatos e conteúdo livre. O envio explícito da tarefa cria um grant limitado ao run, que permite efeitos dentro das raízes, snapshots e regras locais. Em Preferências, escolha também as pastas que o Vox pode acessar; o padrão é Desktop, Documentos, Downloads e, no desenvolvimento, o checkout atual — nunca a pasta pessoal inteira.

Veja [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md), [`docs/SECURITY.md`](docs/SECURITY.md), [`docs/DEPENDENCIES.md`](docs/DEPENDENCIES.md) e o plano em [`.plan/README.md`](.plan/README.md) antes de promover um artefato para instalação.
