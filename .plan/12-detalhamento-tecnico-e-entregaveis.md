# Detalhamento técnico de implementação

Este documento decompõe trabalho técnico além da aparência. Diretórios e tipos são propostas até os spikes; nenhum código abaixo já existe no repositório. As tarefas originais permanecem válidas; T065–T082 acrescentam pacotes de implementação verificáveis.

## 1. Módulos da interface nativa

Estrutura prevista sob `apps/desktop/src/`:

| Módulo | Entrega | Limite de responsabilidade |
|---|---|---|
| app.rs | Inicialização, update e integração dos serviços | Não executar tool nem chamar provider diretamente |
| state.rs | WindowState, ConversationView, VoiceView, RunView | Estado derivado de eventos, sem autoridade de autorização |
| theme.rs | Tokens, escala, claro/escuro, fontes | Única fonte de estilos; sem cores espalhadas |
| window_controller.rs | Invocar, posicionar, recolher, ocultar e monitor | Respeitar capabilities do SO |
| views/conversation.rs | Mensagens, scroll e composer | Texto e Markdown seguro |
| views/activity.rs | Estado, Parar e atividade expandida | Renderizar fatos do broker |
| views/approval.rs | Preview e decisão específica | Não criar nem ampliar permissões |
| views/preferences.rs | Provider, voz, aparência e dados | Chamar serviços de configuração |
| views/history.rs | Consulta/paginação de sessões | Nunca reproduzir efeitos |
| components/ | Botões, campo, menu, tooltip, status e diff | Estados/foco/semântica consistentes |
| bridge.rs | Filas de comandos/eventos e wake-up | Validar payload e sequência |
| accessibility.rs | Nomes, papéis, valores e anúncios | Verificar suporte do toolkit na versão fixada |

O renderer lê ViewModel e emite UiCommand; serviços executam trabalho fora da thread da janela. Repaint solicitado por evento/input, não loop de polling sem necessidade. Limitar mensagens processadas por frame e compactar deltas preservando ordem; Parar tem canal prioritário. Erros de UI nunca habilitam ferramenta bloqueada.

Persistir tema, escala, modo/posição quando permitido, monitor, atalho, preferência de revisão de voz e rascunho por sessão. Não persistir estado “aprovado” como boolean reutilizável nem áudio transitório. Posição inválida deve ser saneada no próximo startup.

## 2. Contratos adicionais UI ↔ serviços

UiCommand propostos: OpenSession, StartTurn, CancelRun, ResolveApproval, BeginCapture, EndCapture, CancelCapture, ChangeModel, SetPreferences, ShowHistory, ExportSession e DeleteSession. Todos com request_id; comandos vinculados ao run incluem run_id/session_id. StartTurn inclui texto final, origem text/voice e modelo selecionado. EndCapture não implica StartTurn quando revisão estiver habilitada.

RunView guarda run_id, estado, resumo da ação, alvo, efeitos confirmados, progresso conhecido, erro e approval pendente. WindowState guarda apenas layout/visibilidade. Desconexão do core mantém histórico visível e bloqueia novo envio até restabelecer handshake. UI não inventa completed quando o processo fecha.

Entregáveis T077: schemas/fixtures para eventos atrasados, duplicados, fora de ordem e overflow; reducer determinístico; política de reconexão; mapeamento completo de estado→texto/controles; testes sem LLM real.

## 3. Storage e migrações

Tabelas propostas: sessions(id,title,created_at,updated_at); messages(id,session_id,role,content,seq); runs(id,session_id,state,model_ref,started_at,finished_at); tool_calls(id,run_id,name,args_redacted,state,effect,verification); approvals(id,run_id,tool_call_id,args_hash,scope,expires_at,state); preferences(key,value,schema_version); session_drafts(session_id,content,updated_at); migrations(version,applied_at). `session_drafts` usa foreign key com cascade, recebe conteúdo já redigido e fica fora da exportação. Definir índices, foreign keys e política de retenção em T078, não guardar blobs ilimitados nos eventos.

Separar mensagem parcial em streaming de mensagem final confirmada; checkpoint limitado para não escrever a cada token. Journal registra intenção antes do efeito e resultado depois; transação do DB não torna efeito de GUI atômico. Após crash, reconciliar unknown por observação ou usuário. Exportação deve preservar ordem e indicar truncamento/redaction. Testar migração N−1→N, backup e falha no meio da migração.

## 4. Percepção e ação de desktop

Pipeline: capability probe → selecionar app/janela → snapshot limitado → redaction → serialização textual → escolha de ação → validar autorização/referência → executar → reobservar → avaliar pós-condição → persistir evidência → comunicar resultado.

T079 deve entregar fixture com botão, toggle, campo de senha, nomes duplicados, diálogo modal, lista virtualizada, nó removido/recriado e mudança de foco. Testes comprovam que handle antigo não vira ação em outro elemento. Se backend não suporta eventos, polling com deadline/backoff e limite; não varrer o desktop inteiro a cada token. Registrar limites de profundidade/nós/tempo como config validada, calibrada por benchmark.

Critério de sucesso é pós-condição específica: app apareceu; valor mudou; item ficou selecionado; arquivo existe com conteúdo esperado. O retorno “press efetuado” sozinho é evidência de dispatch, não resultado da intenção.

## 5. Ferramentas de arquivos, shell e processos

T080 deve entregar uma matriz por tool: schema; risco; recursos afetados; pré-condição; ação; pós-condição; timeout; cancelamento; repetição permitida; resultado parcial. Exemplos:

| Ação | Pré-condição | Pós-condição | Falha/repetição |
|---|---|---|---|
| Criar arquivo | Pai permitido, destino ausente ou overwrite aprovado | Bytes/hash conferidos | Se destino existe após crash, ler antes de repetir |
| Mover arquivo | Origem identificada e destino sem conflito | Destino correto e origem removida | Cross-device pode ser cópia+remoção; relatar parcial |
| Executar testes | Projeto/cwd identificados e ferramenta instalada | Exit code e output suficientes | Repetir só após avaliar efeitos do comando |
| Encerrar processo | PID + identidade/início verificados | Processo original saiu | Não encerrar processo novo que reutilizou PID |
| Abrir app | App resolvido por mecanismo nativo | Janela/app observável ou timeout explícito | Evitar instâncias duplicadas sem necessidade |

Caminhos nunca concatenados em shell sem quoting adequado; preferir argv. Shell arbitrário possui efeitos não inferíveis completamente por parsing; política deve refletir isso. Captura de saída com limite em bytes e política de spill controlado; arquivo temporário não vira canal para ler caminhos arbitrários.

## 6. Runtime, packaging e falhas

Bootstrap inicia storage/política, UI e supervisor; handshake confirma versões antes de habilitar envio. Crash do core não fecha UI; ela mostra “A conexão com o assistente foi interrompida”. Supervisor limita reinícios para evitar crash loop. Encerramento ordenado: bloquear novas ações → cancelar run → parar áudio → esperar workers com deadline → persistir resultados/incertezas → encerrar processos próprios.

T081 entrega mapa de falhas por fronteira (UI, broker, TS, provider, worker, DB, SO), estratégia, timeout e teste de fault injection. T082 entrega smoke do artefato instalado usando a mesma UI planejada; testes de build sem permissões reais não substituem esse aceite.

## 7. Formato obrigatório de evidência

Cada tarefa de implementação registra: versão/commit, SO/compositor, cenário, dados sintéticos, esperado, observado, resultado e link local ao relatório/captura redigida. Pesquisa registra alternativas descartadas e motivo. Não incluir chave, conversa privada ou conteúdo do desktop real em fixtures.

Entregas de UI incluem: capturas reais do egui, mapa de teclado/foco, tabela de estados, medição de contraste dos pares usados e resultado dos UI01–UI16 aplicáveis. Entregas técnicas incluem testes determinísticos das bordas, além de um fluxo real quando dependem do SO. Arquivos do plano não contam como implementação concluída.
