# QA e validação

Este documento separa o que é verificado no checkout do que ainda exige uma conta configurada ou uma sessão gráfica real. Nenhuma linha marcada como pendente é evidência de prontidão de release.

## Suíte local

| Fronteira | Cenário | Evidência |
|---|---|---|
| Protocolo | frames inválidos, limites, versão e `task_grant_id` | `packages/protocol/test`, `crates/protocol`, `tests/contracts` |
| Texto | `TaskSpec`, candidatos e conteúdo livre estruturados; o provider não despacha ferramentas | `packages/provider-adapters/test/provider.test.ts` |
| Jev | `Choice`, `Score`, `Noul`, autenticação, 429, timeout, cancelamento, resposta inválida, opção externa e redação | `packages/provider-adapters/test/provider.test.ts` com servidor local |
| Core | Jev seleciona cada passo; texto só interpreta, propõe candidatos e compõe; baixa confiança reobserva/replaneja | `packages/agent-core/test/core.test.ts` |
| Grant | run divergente, grant finalizado e ferramenta fora do escopo são bloqueados | `crates/broker/src/lib.rs` |
| Broker | snapshots pertencem ao run, expiram e são consumidos por ações; campos acessíveis sensíveis não recebem escrita | `crates/broker`, `crates/desktop-access` |
| Arquivos e processos | raízes canônicas, traversal/symlink, `overwrite`, argv sem shell e identidade de processo | `crates/tool-runtime/src/lib.rs` |
| Dados | migração, keyring, `task_grants`, journal e efeitos `unknown` após interrupção | `crates/session-store/src/lib.rs` |
| Supervisor | NDJSON, cancelamento, heartbeat e recuperação sem repetir um efeito | `crates/supervisor` |
| Interface | preferências dos dois serviços, chaves mascaradas, atividade redigida e ausência de modal de permissão | `apps/desktop/src/main.rs`, `apps/desktop/src/ui_state.rs` |

Execute os gates locais com:

```sh
pnpm typecheck
pnpm test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
pnpm package:dev
pnpm bundle:validate dist/vox-dev
```

## Casos adversariais

| ID | Entrada | Resultado esperado |
|---|---|---|
| A01 | ferramenta desconhecida ou argumentos acima do limite | broker recusa antes do runtime |
| A02 | `TaskGrant` de outro run, vencido ou já finalizado | nenhum efeito |
| A03 | ferramenta não incluída na tarefa submetida | `TaskGrantOutOfScope` |
| A04 | PID não observado, executável não resolvido ou referência de snapshot vencida | nenhum efeito |
| A05 | caminho externo, traversal, symlink externo ou arquivo existente sem `overwrite=true` | runtime recusa; conteúdo original preservado |
| A06 | segredo em tarefa, atividade, contexto ou argumento | redação antes de persistir ou enviar ao serviço |
| A07 | campo de senha, token, chave ou credencial na árvore de acessibilidade | escrita bloqueada |
| A08 | resultado externo incerto ou queda do core durante efeito | journal mantém `unknown`; não há repetição automática |
| A09 | Jev abaixo do limiar, inválido ou indisponível | reobservação e replanejamento; nenhuma ferramenta é repetida |
| A10 | aplicativo Electron sem árvore acessível | instrução para reinício manual com acessibilidade do renderer; sem sucesso inventado |

## Validação manual pendente

- Configurar uma credencial TypeSafe válida, testar o botão **Testar conexão com Jev** e registrar modelo resolvido/latência.
- Configurar um endpoint OpenAI-compatible, testar **Testar conexão de texto** e executar uma jornada controlada: abrir fixture, localizar elemento, escrever, acionar, reobservar e confirmar resultado.
- Repetir uma jornada no Discord somente quando ele expuser a árvore de acessibilidade. O critério é localizar contato, preencher, enviar e observar a confirmação real.
- Validar teclado, IME, escala e Wayland/X11 em cada plataforma publicada.

O Jev e o serviço de texto devem ser medidos em tarefas equivalentes antes de qualquer alegação de ganho de latência, custo ou taxa de sucesso.
