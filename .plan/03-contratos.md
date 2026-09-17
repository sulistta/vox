# Contratos técnicos a implementar

Os nomes deste arquivo são APIs propostas do Vox, não garantias de métodos existentes no Pi ou xa11y. T010 deve transformá-los em schemas executáveis.

## IPC

Proposta: JSON-RPC 2.0 sobre stdin/stdout privados do processo supervisionado, com um objeto JSON por linha, stdout exclusivo do protocolo e logs em stderr. Sem porta TCP pública. Handshake inclui versão, build e capabilities; incompatibilidade interrompe inicialização com diagnóstico. IDs únicos por processo/run, respostas fora de ordem correlacionadas, filas limitadas e limite inicial de mensagem de 1 MiB, ajustável após medições. Dados maiores usam paginação/referências controladas, nunca caminhos arbitrários fornecidos pelo core.

Métodos: `initialize`, `session.open`, `turn.start`, `turn.cancel`, `tool.execute`, `approval.resolve`, `shutdown`. Eventos: `state.changed`, `message.delta`, `tool.started`, `tool.progress`, `tool.completed`, `approval.required`, `run.completed`, `run.failed`, `run.cancelled`. Eventos incluem session_id, run_id, sequência monotônica, timestamp e correlação. Control plane de cancelamento tem prioridade; deltas de UI podem ser agrupados, resultados e aprovações não podem ser descartados silenciosamente.

Erros de framing, EOF, timeout, payload inválido e processo morto devem terminar chamadas pendentes. Heartbeat e watchdog detectam perda de responsividade. Reiniciar core não retoma ações automaticamente.

## Ferramentas

Envelope de chamada: `call_id`, `session_id`, `run_id`, `tool`, `arguments`, `deadline`, `authorization_ref` opcional. Resultado: `status` (success/error/cancelled/unknown), `data`, `error_code`, `retryable`, `side_effect` (none/applied/unknown), `duration_ms`, `truncated`, `continuation` opcional, `verification`.

Broker confere schema e limites antes de autorização e execução. Catálogo registra descrição, schema, risco, plataformas, capability necessária, tipo de efeito e estratégia de verificação. Aprovação gerada pelo broker associa hash dos argumentos, recurso, escopo e expiração; o LLM não consegue fabricar autorização.

| Grupo | Ferramentas propostas | Verificação |
|---|---|---|
| Desktop | capabilities, list_apps, list_windows, snapshot, query, act, wait_for | Reconsulta e pós-condição semântica |
| Arquivos | list, search, read, write, mkdir, move, trash | Metadados/hash, destino e conflitos |
| Shell | exec, read_output, cancel | Código de saída, stderr e pós-condição da tarefa |
| Processos | list, inspect, terminate | Identidade do processo e saída confirmada |
| Apps/janelas | launch, focus, open_path | App/janela observável, sem inferir apenas pelo spawn |
| Clipboard | read, write | MIME/tamanho e leitura quando permitida |

Não expor exclusão permanente por padrão. Shell usa programa + argv quando possível; shell livre é explicitamente classificado como execução arbitrária, não “seguro” por filtro de strings. Saídas têm limite e paginação; comandos podem ter timeout configurado por execução. Redação de segredos ocorre antes de retornar ao modelo.

## Snapshot semântico

Campos: snapshot_id, captured_at, generation, app_id, window_id, focused_element, capabilities, nodes, truncated e continuation. Cada nó: element_ref opaco, parent_ref, role, name, value redigido, states, actions, description e bounds opcionais. Não serializar handles/pointers nativos nem campos de senha. Texto e nomes são conteúdo não confiável.

Referências são vinculadas a app/janela/geração. O broker guarda cada snapshot que emite e o modelo só devolve `snapshot_id`, nunca uma árvore serializada como autorização; a referência expira rapidamente, é vinculada ao run e é consumida por uma ação. Antes do efeito: resolver novamente, verificar identidade, existência, estado e política. Elemento obsoleto retorna STALE_ELEMENT e exige nova observação. Ambiguidade não escolhe primeiro candidato silenciosamente. Lista virtualizada exige navegação/paginação limitada; indicar o que não foi observado.

## Execução e cancelamento

Estados: idle → receiving/transcribing → thinking → executing ↔ thinking; waiting_user/awaiting_approval suspendem efeitos; completed, failed e cancelled são terminais. Cancelling bloqueia novas ferramentas imediatamente, cancela provider e sinaliza workers. Ação nativa já entregue pode concluir: relatar efeito aplicado ou incerto; não prometer desfazer tudo.

Run mantém objetivo, ferramentas pendentes, orçamento de passos/tempo/tokens, ações confirmadas e fatos verificados. Proposta inicial: até 30 ações e 10 minutos por run antes de solicitar continuação, ambos configuráveis. Progresso repetido sem mudança por três ciclos encerra ou pede orientação. Retry com backoff para leituras/falhas transitórias; efeitos incertos exigem observação ou intervenção.

Execuções pendentes na recuperação viram interrupted/unknown. Deduplicar call_id na sessão viva e persistir intenção/resultado, sem prometer exactly-once para interfaces gráficas. Pós-condição e idempotência determinam se repetir é permitido.
