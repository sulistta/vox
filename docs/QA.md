# QA, adversarial e fault injection

Este documento é a matriz operacional de T050, T051, T052, T054, T074, T075 e
T081. Ele separa o que é determinístico e executável no checkout do que
precisa de sessão gráfica, outro sistema, hardware, conta de provider ou uso
diário. Uma linha `pendente` não é um resultado aprovado.

## Suíte determinística local

| Fronteira | Cenário | Evidência | Estado |
|---|---|---|---|
| Contrato | JSON inválido, tipo desconhecido, versão incompatível e frame acima do limite | `packages/protocol/test`, `crates/protocol` e `tests/contracts` | passou |
| Provider | SSE, cancelamento, erro de autenticação/rede, fallback JSON, shape de tool, stream interrompido e evento sem delimitador limitado | `packages/provider-adapters/test/provider.test.ts` | passou para servidor local |
| Provider remoto controlado | conversa textual, cancelamento, abertura de sessão e atividade redigida; ferramentas bloqueadas | `scripts/provider-smoke.mjs` contra endpoint OpenAI-compatible remoto | passou em 17/09/2026: `conversation` (2,322 s) e `cancel` (76 ms), sem tool request; não mede qualidade/custo/rate limit |
| Decisão de modelo | decisão JSON estrita, catálogo descritivo, resultado de ferramenta na rodada seguinte e redação antes do provider | `packages/provider-adapters/test/provider.test.ts`, `packages/agent-core/test/core.test.ts` | passou com servidor HTTP local |
| Core | handshake, sessão, streaming, pedido de tool, correlação de `call_id`, cancelamento, configuração inválida estruturada e evento `model.requested` igual ao payload redigido | `packages/agent-core/test/core.test.ts`, `crates/supervisor/tests/supervisor.rs` | passou |
| Voz (contrato) | duração máxima, cancelamento, áudio/transcrição vazios, falha de permissão e revisão antes do turno | `packages/agent-core/test/voice.test.ts` | passou sem captura/STT nativos |
| Voz (captura) | PCM bounded, pico de nível, key-up, cancelamento por foco e watchdog/limite | `crates/audio-capture/src/lib.rs` | passou em fixture; microfone real não foi gravado |
| E2E | core TypeScript → supervisor NDJSON → broker/policy → runtime de clipboard → resultado aprovado → core terminal | `crates/supervisor/tests/e2e.rs` | passou em fixture Linux |
| Política | aprovação ausente, hash divergente, expiração e replay single-use | `crates/policy/src/lib.rs`, `crates/broker/src/lib.rs` | passou |
| Efeito de arquivo | raiz permitida, traversal, symlink externo, overwrite e rename atômico | `crates/tool-runtime/src/lib.rs` | passou para casos estáticos; troca concorrente de symlink/diretório entre validação e operação ainda bloqueia aprovação de segurança |
| Saída/processo | argv sem interpolação shell, ambiente mínimo, saída grande, timeout, grupo privado, identidade PID/comando/start-time e recusa de mismatch | `crates/tool-runtime/src/lib.rs` | passou localmente |
| Supervisor ruidoso | EOF inesperado e linha NDJSON inválida do core geram diagnóstico, sem travar o chamador; turno lento que responde heartbeat não é reiniciado | `crates/supervisor/tests/supervisor.rs`, `crates/supervisor/tests/fixtures/{exit,invalid,slow-turn}.mjs` | passou localmente |
| Crash com efeito pendente | queda do core marca o efeito do run como `unknown`, e a atualização posterior para sucesso é recusada | `crates/session-store/src/lib.rs`, `apps/desktop/src/main.rs` | passou localmente |
| Dados | redação, idempotência, migration/reopen, recovery, diagnóstico de corrupção, approvals auditáveis, rascunho por sessão e bloqueio de exclusão de run ativo | `crates/session-store/src/lib.rs` | passou localmente; o teste lê SQLite e backup diretamente |
| Journal de efeitos | intenção `pending`, resultado terminal, `pending`→`unknown` no reopen e bloqueio de segundo completion | `crates/session-store/src/lib.rs`, `apps/desktop/src/main.rs` | passou localmente; reconciliação externa ainda não |
| Lease global do desktop | segunda instância é recusada; heartbeat, release e recuperação após timeout | `crates/session-store/src/lib.rs`, `apps/desktop/src/main.rs` | passou em duas conexões SQLite; crash real do processo continua externo |
| Contexto/modelo | contexto bounded preserva restrições/unknown; perfil e probe não inventam capabilities | `packages/agent-core/src/context.ts`, `packages/provider-adapters/src/index.ts` | passou com provider fake e servidor HTTP local |
| Instalador Linux | manifesto/SBOM, cópia atômica, launcher, preservação e remoção explícita de dados | `packaging/linux`, `scripts/validate-bundle.sh` | passou no smoke do bundle dev; não é instalador Windows/macOS |
| Semântica | alvo ambíguo, referência stale, snapshot vinculado ao run/consumido pela ação, toggle, campos sensíveis em português/inglês redigidos e bloqueados, limite e escopo opcional por `app_pid` | `crates/desktop-access/src/lib.rs`, `crates/broker/src/lib.rs` | passou na fixture e no registry do broker |
| Acessibilidade AT-SPI real | fixture GTK: PID direcionado, duplicidade, senha e token em português redigidos/bloqueados, dispatch `unknown`, espera, nó recriado/ref stale, truncamento e diálogo | `crates/desktop-access/tests/native_fixture.rs` | passou em GNOME/Wayland nesta máquina; requer sessão gráfica |
| Janela Vox por AT-SPI | labels básicos, compositor `text_area` associado a “Mensagem”, Recolher → Expandir, menu pressionável, Preferências e opções Sistema/Claro/Escuro | `apps/desktop/tests/native_accessibility.rs` | passou em GNOME/Wayland com o status de leitor de tela já ativo; é teste opt-in |
| UI | streaming sem duplicação, compacto preservando run, Recolher mantendo a única janela visível, erro/terminalidade e descarte de eventos stale por `run_id`; tema Sistema/Claro/Escuro, decisão com rótulo específico, bloqueio de Enter durante aprovação e retirada da bolha otimista em falha de dispatch | `apps/desktop/src/main.rs`, `apps/desktop/src/ui_state.rs` | passou em testes unitários e no smoke AT-SPI aplicável |

## Casos adversariais obrigatórios

Cada caso abaixo deve permanecer bloqueado ou produzir diagnóstico explícito.

| ID | Entrada | Resultado exigido |
|---|---|---|
| A01 | tool desconhecida ou argumentos acima de 64 KiB | broker rejeita sem chamar runtime |
| A02 | approval com outro `tool`, hash, escopo, expiração ou UUID inválido | `INVALID_APPROVAL`; nenhum efeito |
| A03 | replay do mesmo approval | token não encontrado após o primeiro uso |
| A04 | caminho absoluto fora da raiz, `..` ou symlink para fora | `OutOfScope` |
| A05 | destino existente sem `overwrite` aprovado | `DestinationExists`; conteúdo original preservado |
| A06 | conteúdo que parece segredo em mensagem, efeito, rascunho, export ou backup | valor redigido; o sentinela não aparece em `session_drafts.content` nem no SQLite de backup |
| A07 | PID próprio ou identidade diferente da observada | término recusado |
| A08 | stdout/stderr maior que o limite | processo não trava por pipe cheio; saída marcada truncada |
| A09 | evento atrasado, `call_id` errado ou terminal duplicado | correlação rejeitada; estado terminal não reabre |
| A10 | crash antes/depois do efeito | run interrompido/unknown; no máximo três reinícios do core e não repetir automaticamente |

Os A01–A10 agora têm testes nomeados no CI local, distribuídos entre política,
broker, runtime, supervisor e session-store. A10 prova a fronteira de crash
do armazenamento e a não repetição do efeito idempotente; fault injection no
processo empacotado e jornada real de efeito continuam necessários para o
aceite de T054/T081.

O E2E em `crates/supervisor/tests/e2e.rs` atravessa o processo real do core e
o supervisor, usando o broker Rust para leitura de clipboard e para uma escrita
que passa por `APPROVAL_REQUIRED` antes do fixture receber os bytes. Ele não é
prova de Wayland/X11 real nem substitui o smoke do pacote instalável.

`pnpm benchmark:core` mede startup, turno textual fake e RSS do processo
privado no hardware atual. O resultado é uma linha de base reproduzível, não a
meta de produto: não inclui renderização egui, provider de rede, modelo local,
STT, custo nem uma matriz p50/p95 entre plataformas.

## Provider e modelos

O provider fake é determinístico e o adapter OpenAI-compatible é testado contra
um servidor HTTP local que emite SSE. Além disso, em 17/09/2026 um smoke
controlado contra um endpoint remoto OpenAI-compatible com o modelo
`nex-agi/nex-n2.5-pro:free` concluiu os modos `conversation` e `cancel`. O core
abriu sessão, emitiu a atividade redigida e terminou no estado esperado; o
script bloqueou qualquer pedido de ferramenta. O resultado não avalia
qualidade, disponibilidade, custo, rate limit, latência p95 ou jornadas de
ferramenta em aplicativos reais.

Depois de `pnpm build`, execute o smoke somente com variáveis efêmeras já
presentes no ambiente; não grave a chave em arquivo nem a cole no comando:

```sh
pnpm smoke:provider
VOX_PROVIDER_SMOKE_MODE=cancel pnpm smoke:provider
```

O JSON final contém apenas metadados redigidos: provider, referência de modelo,
modo, tipos de evento, terminal, latência, contagem de caracteres e contagem
de blocos de stderr. Ele nunca espelha o payload, a resposta nem stderr do
provider.

Para fechar T051, registrar por combinação:

```text
build, provider_id, endpoint/model_ref, parâmetros, tokens/custo se aplicável,
SO/compositor, jornada, pré-condição, resultado, latência p50/p95,
falha/retry e evidência redigida
```

Payload multimodal não faz parte da v1 textual. Um modelo sem tool calling só
pode usar o fallback JSON validado; texto que não passa pelo parser é dado,
nunca execução.

## Jornada E2E e interface

Os testes automáticos não substituem a janela egui real. T052/T075/T081 devem
executar, em cada plataforma publicada, as jornadas J01–J08 e UI01–UI16 com:

- estado inicial conhecido e fixture acessível controlada;
- captura do modo invocação, conversa, compacto, aprovação e erro;
- teclado, IME, scroll, foco, 100/150/200% e larguras 320/360/440/640;
- aprovação explícita, Parar e reconexão;
- resultado de pós-condição, distinguindo `success`, `unknown`, cancelado e
  dispatch sem verificação.

Nesta execução, a fixture GTK controlada foi acionada no AT-SPI real. A própria
janela Vox também expôs labels, menu e Preferências quando o status
`ScreenReaderEnabled` do sistema estava ativo; a mudança temporária usada para
a prova foi restaurada ao final. O teste do checkout não altera esse status:
ele apenas verifica uma sessão em que o leitor de tela já está ativo. Isso não
substitui o aceite visual, de teclado, IME, escala ou multiplataforma.

Um smoke visual isolado em Xvfb abriu a invocação clara e escura de 440×280. A primeira
captura revelou que os 176 pt previstos inicialmente cortavam o compositor e
que o rótulo do modelo colidia com os botões; o layout e a geometria foram
corrigidos e a captura seguinte não mostrou controles cortados. A captura não
cobre conversa, aprovação, escala ou uma sessão Wayland real.

## Comandos

```sh
pnpm typecheck
pnpm test
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build -p vox-desktop
cargo run -p vox-desktop-access --example native_probe
TMPDIR=/tmp cargo test -p vox-desktop-access --test native_fixture -- --ignored --nocapture
VOX_TEST_A11Y=1 cargo test -p vox-desktop --test native_accessibility -- --ignored --nocapture
```

O segundo teste de sessão gráfica exige Linux, AT-SPI e
`org.a11y.Status.ScreenReaderEnabled=true`; ele não habilita nem desabilita a
preferência do sistema.

Os relatórios desta execução estão em [16/09](../.plan/EXECUCAO-2026-09-16.md) e [17/09](../.plan/EXECUCAO-2026-09-17.md).
