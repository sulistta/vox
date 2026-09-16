# Backlog executável

Todas as tarefas estão pendentes. As responsabilidades são papéis técnicos, não pessoas já alocadas. Dependências referem-se a IDs abaixo. Cada tarefa exige evidência de aceite, revisão de segurança proporcional e documentação alterada quando necessário.

## Definition of Done comum

Código revisado, compilação/lint aplicáveis aprovados, testes do comportamento relevante, erros e cancelamento tratados, nenhum segredo em logs, contratos atualizados, compatibilidade registrada e critério específico demonstrado. Tarefas de pesquisa entregam relatório/ADR com comandos, versões, resultados e conclusão; não exigem código de produção. Tarefas documentais exigem revisão de consistência, sem testes artificiais.

## Índice

| ID | Tarefa | Prioridade | Esforço | Dependências |
|---|---|---|---|---|
| T001 | Alinhar referências de produto | P0 | P | — |
| T002 | Provar runtime Rust/TypeScript | P0 | M | T001 |
| T003 | Auditar e fixar o upstream Pi | P0 | M | T001 |
| T004 | Provar xa11y e semântica dos apps | P0 | M | T001 |
| T005 | Provar Linux Wayland e X11 | P0 | M | T004 |
| T006 | Provar Windows | P0 | M | T004 |
| T007 | Provar macOS | P0 | M | T004 |
| T008 | Fechar baseline técnica e gate G0 | P0 | P | T002,T003,T005,T006,T007 |
| T009 | Criar workspace e UI mínima | P0 | M | T008 |
| T010 | Definir schemas e tipos compartilhados | P0 | M | T008 |
| T011 | Implementar IPC e supervisor | P0 | G | T009,T010 |
| T012 | Criar fork seletivo | P0 | G | T003,T009 |
| T013 | Adaptar loop e eventos de agente | P0 | G | T010,T012 |
| T014 | Implementar broker e política básica | P0 | G | T010,T011 |
| T015 | Preparar CI e gestão de dependências | P1 | M | T009,T012 |
| T016 | Eliminar bypass de ferramentas herdadas | P0 | M | T013,T014 |
| T017 | Integrar provider comercial textual | P0 | M | T013,T016 |
| T018 | Integrar provider local e endpoint customizado | P0 | M | T017 |
| T019 | Suportar diferenças entre modelos | P0 | G | T017,T018 |
| T020 | Orquestrar runs e limites | P0 | M | T016,T017 |
| T021 | Persistir sessões e recuperar histórico | P1 | G | T013,T020 |
| T022 | Guardar segredos e controlar retenção | P0 | M | T017,T021 |
| T023 | Implementar parada ponta a ponta | P0 | G | T011,T020 |
| T024 | Criar adapter Rust de acessibilidade | P0 | G | T004,T010,T014 |
| T025 | Normalizar e limitar snapshots | P0 | M | T024 |
| T026 | Resolver alvos e consultas | P0 | M | T025 |
| T027 | Executar ações semânticas | P0 | G | T014,T026 |
| T028 | Verificar resultados e navegar apps reais | P0 | G | T027 |
| T029 | Recuperar falhas e impedir loops | P0 | M | T020,T028 |
| T030 | Implementar shell controlado | P0 | G | T014,T020 |
| T031 | Implementar leitura e busca de arquivos | P0 | M | T014 |
| T032 | Implementar escrita e organização | P0 | G | T031 |
| T033 | Abrir aplicativos e caminhos | P0 | M | T014,T024 |
| T034 | Gerenciar processos | P1 | M | T030 |
| T035 | Controlar janelas e clipboard | P1 | M | T024,T033 |
| T036 | Integrar escolha de ferramentas | P0 | M | T019,T029,T030,T032,T033,T034,T035 |
| T037 | Construir conversa e streaming nativos | P0 | G | T009,T013,T020 |
| T038 | Integrar Parar e aprovações à UI | P0 | M | T014,T023,T027,T037,T068 |
| T039 | Criar onboarding e configurações | P1 | M | T022,T037 |
| T040 | Polir janela residente e acessibilidade | P1 | G | T037,T038 |
| T041 | Validar alpha com usuários de teste | P1 | M | T021,T036,T039,T040,T069,T070,T071,T072,T074,T076,T077,T078,T079,T080 |
| T042 | Escolher transcrição de voz | P1 | M | T008 |
| T043 | Implementar captura push-to-talk | P1 | G | T040,T042 |
| T044 | Integrar STT e turno textual | P1 | G | T020,T043 |
| T045 | Tratar atalhos e foco de voz por SO | P1 | M | T044,T005,T006,T007 |
| T046 | Avaliar voz em português | P1 | M | T044,T045 |
| T047 | Consolidar suporte Linux | P0 | G | T035,T040,T045 |
| T048 | Consolidar suporte Windows | P0 | G | T035,T040,T045 |
| T049 | Consolidar suporte macOS | P0 | G | T035,T040,T045 |
| T050 | Executar suíte adversarial | P0 | G | T022,T029,T032,T038 |
| T051 | Avaliar providers e modelos | P0 | M | T019,T021,T036 |
| T052 | Automatizar jornadas E2E | P0 | G | T028,T036,T038,T047,T048,T049,T051 |
| T053 | Medir desempenho e consumo | P1 | M | T046,T047,T048,T049 |
| T054 | Testar falhas e recuperação | P0 | G | T021,T023,T030,T044 |
| T055 | Criar artefatos e instaladores | P1 | G | T015,T047,T048,T049 |
| T056 | Validar instalação e atualização | P1 | M | T021,T055 |
| T057 | Executar beta de uso diário | P1 | G | T041,T046,T050,T052,T053,T054,T056,T075,T081,T082 |
| T058 | Preparar documentação e release | P1 | M | T057 |
| T059 | Definir manutenção e suporte | P1 | M | T015,T058 |
| T060 | Auditar conclusão e publicar v1 | P0 | M | T058,T059 |
| T061 | Avaliar OCR e visão opcionais | P2 | G | T060 |
| T062 | Avaliar TTS e wake word | P2 | M | T060 |
| T063 | Avaliar extensibilidade e automações | P2 | G | T060 |
| T064 | Avaliar memória e sincronização | P2 | G | T060 |
| T065 | Especificar e prototipar a janela flutuante | P0 | M | T001 |
| T066 | Implementar controlador da janela | P0 | G | T009,T065 |
| T067 | Implementar tokens e componentes nativos | P0 | M | T009,T065 |
| T068 | Implementar compositor e histórico de conversa | P0 | G | T037,T066,T067 |
| T069 | Implementar modo compacto e visibilidade | P0 | M | T023,T038,T066 |
| T070 | Implementar navegação de sessões | P1 | M | T021,T068 |
| T071 | Implementar preferências e onboarding completos | P1 | M | T039,T067 |
| T072 | Implementar atividade, aprovação e falha parcial | P0 | G | T038,T068 |
| T073 | Implementar experiência visual e acessível de voz | P1 | M | T044,T068 |
| T074 | Validar foco, teclado, escala e acessibilidade | P0 | G | T068,T069,T071,T072 |
| T075 | Criar suíte de estados e fluxos da interface | P1 | G | T068,T070,T072,T073,T074 |
| T076 | Polir responsividade e consumo da janela | P1 | M | T040,T074 |
| T077 | Implementar reducer e contratos de apresentação | P0 | M | T010,T020,T068 |
| T078 | Detalhar e testar dados e migrações | P1 | M | T021,T022 |
| T079 | Construir fixture semântica adversarial | P0 | G | T025,T027 |
| T080 | Consolidar matriz de efeitos das ferramentas | P0 | M | T030,T032,T034 |
| T081 | Validar falhas entre UI, core e workers | P0 | G | T054,T077,T078 |
| T082 | Validar experiência no pacote instalado | P1 | M | T055,T056,T075,T081 |

## Tarefas detalhadas

### T001 — Alinhar referências de produto

- [x] Concluída
- **Prioridade / esforço:** P0 / P. **Responsabilidade:** Produto.
- **Depende de:** —.
- **Trabalho e entrega:** Registrar nome provisório, modelos/apps de referência, hardware, orçamento e recorte das jornadas.
- **Aceite:** R01–R11 e J01–J08 revisados; decisões abertas registradas sem bloquear escolhas reversíveis.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída.
- **Evidências:** [T001 — alinhamento de referências de produto](T001-alinhamento-produto.md); levantamento somente leitura do ambiente Linux registrado no documento; R01–R11 e J01–J08 conferidos contra `01-produto.md`.

### T002 — Provar runtime Rust/TypeScript

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Arquitetura.
- **Depende de:** T001.
- **Trabalho e entrega:** Comparar processo próprio com runtime empacotado e embedding; medir startup, tamanho, streaming, crash e cancelamento.
- **Aceite:** Protótipo reproduzível sem executável Pi; A01 decidida com dados e build mínimo nos três SOs.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; evidência: [T002 — runtime spike](T002-runtime-spike.md). Processo privado foi medido e escolhido provisoriamente; embedding e instaladores nos três SOs continuam abertos.

### T003 — Auditar e fixar o upstream Pi

- [x] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T001.
- **Trabalho e entrega:** Resolver upstream, fixar SHA, inventariar módulos/dependências, licença, acoplamento e custo do fork.
- **Aceite:** Manifesto de proveniência com candidatos a reutilização e exclusão; estratégia de fork seletivo definida.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída para auditoria/proveniência; evidência: [T003 — auditoria do upstream Pi](T003-upstream-audit.md). O fork seletivo de código ainda é T012.

### T004 — Provar xa11y e semântica dos apps

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Desktop.
- **Depende de:** T001.
- **Trabalho e entrega:** Fixar versão; observar e acionar fixture, VS Code e Discord; medir árvores, threading, falhas e chamadas bloqueantes.
- **Aceite:** Relatório por app com ações realmente disponíveis e limite conhecido; Rust integra sem modificar xa11y.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; evidência: [T004 — xa11y e semântica](T004-xa11y-semantics.md) e [T021–T025 — adapter nativo](T021-T025-dados-acessibilidade.md). Probe real Linux passou; VS Code/Discord/fixture acionável e três SOs não foram provados.

### T005 — Provar Linux Wayland e X11

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T004.
- **Trabalho e entrega:** Testar GNOME/KDE Wayland e X11: AT-SPI, foco, atalhos, clipboard, residência e apps sandboxed.
- **Aceite:** Matriz por ambiente separa semântica de input/foco e define fallback viável para cada lacuna.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; GNOME/Wayland observado e backend de clipboard coberto por fixture; X11/KDE, foco cross-compositor, clipboard real e sandbox matrix estão registrados como pendentes em [PLATFORM-MATRIX](../docs/PLATFORM-MATRIX.md).

### T006 — Provar Windows

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T004.
- **Trabalho e entrega:** Testar UIA, níveis de integridade, spawn, janela, atalhos, áudio e distribuição experimental.
- **Aceite:** Fixture acessível funciona com usuário normal; UAC e apps elevados têm limitações registradas.
- **Execução:** status: bloqueada por ambiente externo nesta sessão; workflow de build configurado em `.github/workflows/ci.yml`, sem declarar UIA ou fixture Windows como sucesso.

### T007 — Provar macOS

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T004.
- **Trabalho e entrega:** Testar permissões por versão, bundle, foco, áudio, atalhos e APIs exigidas pela dependência.
- **Aceite:** App empacotado observa/aciona fixture e orienta concessão/revogação de permissões.
- **Execução:** status: bloqueada por ambiente externo nesta sessão; workflow de build configurado em `.github/workflows/ci.yml`, sem declarar AXUIElement ou fixture macOS como sucesso.

### T008 — Fechar baseline técnica e gate G0

- [ ] Concluída
- **Prioridade / esforço:** P0 / P. **Responsabilidade:** Arquitetura.
- **Depende de:** T002,T003,T005,T006,T007.
- **Trabalho e entrega:** Fixar toolchains, SOs/arquiteturas de referência, runtime, versões, metas e riscos restantes.
- **Aceite:** ADRs e matriz publicada; bloqueios centrais têm solução demonstrada ou decisão explícita de produto.
- **Execução:** status: parcial; toolchain, locks, limites e riscos estão documentados, mas G0 permanece aberto até T005–T007 e matriz de distribuição real.

### T009 — Criar workspace e UI mínima

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Desktop.
- **Depende de:** T008.
- **Trabalho e entrega:** Criar Cargo workspace, packages TS, manifests, lockfiles, lint e janela egui com fila assíncrona.
- **Aceite:** Build e janela mínima nos três SOs; nenhuma dependência Tauri/Electron/WebView.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; evidência: [T009 — workspace e UI](T009-workspace-ui.md). Build Linux e processo nativo passaram; três SOs e janela AT-SPI própria ainda não.

### T010 — Definir schemas e tipos compartilhados

- [x] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Arquitetura.
- **Depende de:** T008.
- **Trabalho e entrega:** Implementar contratos de IPC, ferramentas, snapshots, erros e capabilities com geração/validação cruzada.
- **Aceite:** Fixtures válidas e inválidas passam testes TS/Rust; versão incompatível é rejeitada.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída; evidência: [T010 — contratos cross-language](T010-contratos-cross-language.md), fixtures TS/Rust compartilhadas e rejeição de versão incompatível.

### T011 — Implementar IPC e supervisor

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop.
- **Depende de:** T009,T010.
- **Trabalho e entrega:** Criar pipes privados, handshake, correlação, heartbeat, limites, shutdown e recuperação.
- **Aceite:** Core morto/ruidoso não trava UI; chamadas pendentes terminam; stdout inválido é diagnosticado.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; evidência: [T011 — IPC e supervisor](T011-supervisor-ipc.md). Handshake/correlação/fechamento/heartbeat/EOF/watchdog de pendências e até três reinícios passam; reconciliação de efeitos desconhecidos após crash ainda está aberta.

### T012 — Criar fork seletivo

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Core.
- **Depende de:** T003,T009.
- **Trabalho e entrega:** Preservar origem, importar módulos selecionados, remover CLI/TUI e acoplamentos de produto.
- **Aceite:** Core próprio inicia sem Pi instalado e sem ler configurações/extensões do Pi do usuário.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; core próprio já inicia sem Pi instalado/configuração do usuário; extração formal de módulos Pi ainda não foi incorporada.

### T013 — Adaptar loop e eventos de agente

- [x] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Core.
- **Depende de:** T010,T012.
- **Trabalho e entrega:** Expor mensagens, deltas, chamadas de ferramenta, resultados, erros e abort do core próprio.
- **Aceite:** Provider fake percorre conversa→tool→resposta; abort e erro são eventos estruturados.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída para provider fake e tool flow; evidência: [T013–T020 — core e broker](T013-T020-core-broker.md) e `packages/agent-core/test/core.test.ts`.

### T014 — Implementar broker e política básica

- [x] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Segurança.
- **Depende de:** T010,T011.
- **Trabalho e entrega:** Validar chamadas, escopos, argumentos, aprovação vinculada e expiração; aplicar política antes do efeito.
- **Aceite:** Tool adulterada, replay e efeito fora do escopo bloqueados; autorização válida não repete perguntas.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída para política/broker básico; evidência: [T013–T020 — core e broker](T013-T020-core-broker.md), testes de expiração, hash e replay.

### T015 — Preparar CI e gestão de dependências

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Entrega.
- **Depende de:** T009,T012.
- **Trabalho e entrega:** Configurar fmt/lint/test/build por SO, caches, inventário de licenças e revisão de upgrades.
- **Aceite:** Pipeline reproduzível a partir dos lockfiles; relatório de proveniência acompanha fork.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; evidência: `.github/workflows/ci.yml`, [DEPENDENCIES](../docs/DEPENDENCIES.md), `rust-toolchain.toml` e lockfiles; execução remota da matriz ainda pendente.

### T016 — Eliminar bypass de ferramentas herdadas

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T013,T014.
- **Trabalho e entrega:** Adaptar ferramentas reaproveitadas para chamar broker e remover caminhos diretos de efeitos.
- **Aceite:** Auditoria e teste provam shell/arquivos/GUI passam pela mesma política; G1 atendido.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; ferramentas de arquivos, shell, processos, apps e GUI entram pelo broker, mas auditoria adversarial completa G1 segue em T050.

### T017 — Integrar provider comercial textual

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T013,T016.
- **Trabalho e entrega:** Escolher provider de referência e implementar auth, streaming, chamadas estruturadas e erros.
- **Aceite:** Turno com tool real funciona sem imagem; falhas de credencial, rate limit e timeout são legíveis.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; adapter OpenAI-compatible e SSE fake local passam, mas não foi usada credencial/conta comercial real.

### T018 — Integrar provider local e endpoint customizado

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T017.
- **Trabalho e entrega:** Adicionar endpoint configurável, capability probing e modelo local de referência.
- **Aceite:** Mesma jornada roda local sem provider comercial; endpoint indisponível não causa loop.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; endpoint customizado é configurável e testado contra servidor local; engine local dedicada e probe de capabilities permanecem abertos.

### T019 — Suportar diferenças entre modelos

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Core.
- **Depende de:** T017,T018.
- **Trabalho e entrega:** Normalizar tools/contexto, fallback JSON validado, troca segura de provider e compactação.
- **Aceite:** Modelo sem tools nativas pode executar fixture por JSON; saída inválida nunca vira ação; incompatibilidade é explícita.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; fallback JSON validado e rejeição de shape perigoso passam; compactação/troca real de modelo ainda não.

### T020 — Orquestrar runs e limites

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T016,T017.
- **Trabalho e entrega:** Implementar estados, uma execução com efeitos, filas, orçamentos e vínculo da intenção às tools.
- **Aceite:** Estados terminais consistentes; limite e aprovação pausam efeitos; duas sessões não disputam desktop.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; estados terminais, run busy, timeout, orçamento de prompt/saída/steps/duração, estagnação, vínculo de call e bloqueio de dois runs concorrentes na mesma sessão persistida estão implementados; arbitragem de desktop entre múltiplas sessões e reconciliação de unknown ainda seguem abertos.

### T021 — Persistir sessões e recuperar histórico

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** Core.
- **Depende de:** T013,T020.
- **Trabalho e entrega:** Criar DB versionado, transações, journal, migrações e recuperação de run interrupted.
- **Aceite:** Reinício preserva mensagens confirmadas e não repete efeitos; corrupção recebe diagnóstico recuperável.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; evidência: [T021–T025 — dados e acessibilidade](T021-T025-dados-acessibilidade.md). SQLite, migração, recovery, diagnóstico de corrupção, retenção determinística, journal idempotente e arbitragem de um run ativo por sessão passam; backup/migração interrompida do instalador ainda não.

### T022 — Guardar segredos e controlar retenção

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Segurança.
- **Depende de:** T017,T021.
- **Trabalho e entrega:** Integrar keyring, redaction, exclusão/exportação e aviso de envio remoto.
- **Aceite:** Segredo não aparece em logs/DB/exportação; keyring indisponível não gera arquivo plaintext implícito.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; redaction, exportação sem credenciais, aviso de provider configurado, retenção configurável com proteção da sessão atual, adaptador `vox-secrets` com libsecret/fixture e formulário básico de conta + campo mascarado passam; o segredo só é injetado no ambiente do core privado e nunca no SQLite. Security.framework/Windows Credential Manager, endpoint/modelo editáveis e aplicação sem reabrir o core continuam pendentes.

### T023 — Implementar parada ponta a ponta

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop.
- **Depende de:** T011,T020.
- **Trabalho e entrega:** Priorizar cancelamento na UI, core, provider e workers; revogar aprovações e impedir novos efeitos.
- **Aceite:** Parar funciona mesmo com provider/worker lento; reporta ação aplicada/incerta sem promessa de rollback.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; abort do core, espera de tool, revogação de approval, botão Parar, watchdog de requests e encerramento de grupo privado de subprocessos têm cobertura local; provider/worker empacotado lento, pós-condição externa e medição de latência ainda faltam.

### T024 — Criar adapter Rust de acessibilidade

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop.
- **Depende de:** T004,T010,T014.
- **Trabalho e entrega:** Encapsular xa11y, tratar permissões, threading, ciclo de vida e capabilities por ambiente.
- **Aceite:** Fixture é enumerada nos três SOs; falhas nativas não derrubam GUI.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; evidência: [T021–T025 — adapter nativo](T021-T025-dados-acessibilidade.md). xa11y 0.14.0 integrado e probe Linux real passa; fixture/Windows/macOS ainda abertos.

### T025 — Normalizar e limitar snapshots

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Desktop.
- **Depende de:** T024.
- **Trabalho e entrega:** Criar árvore textual com referências opacas, redaction, paginação, foco e truncamento explícito.
- **Aceite:** Árvore grande cabe no orçamento; senhas não são expostas; modelo recebe apenas dados textuais.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; evidência: [T021–T025 — snapshots](T021-T025-dados-acessibilidade.md), limites 512/8, refs opacas, foco, truncamento e redaction.

### T026 — Resolver alvos e consultas

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Desktop.
- **Depende de:** T025.
- **Trabalho e entrega:** Implementar busca por papel/nome/estado, escopo app/janela, listas virtualizadas e referências temporárias.
- **Aceite:** Ambiguidade e stale refs geram erro recuperável; nenhum alvo arbitrário é selecionado.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; fixture e query semântica cobrem papel/nome/estado/ação, limite/paginação, ambiguidade e stale ref; escopo rico por app/janela e lista virtualizada nativa ainda faltam.

### T027 — Executar ações semânticas

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop.
- **Depende de:** T014,T026.
- **Trabalho e entrega:** Implementar ações suportadas de ativar, editar, selecionar, alternar, expandir e focar com revalidação.
- **Aceite:** Fixture muda como esperado; ação indisponível é explícita; nenhuma dependência de screenshot.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; fixture cobre press/toggle/set-value/foco e adapter nativo despacha ações após revalidar app/janela/referência no snapshot; pós-condições reais ainda são explicitamente `unknown` quando não podem ser observadas.

### T028 — Verificar resultados e navegar apps reais

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop.
- **Depende de:** T027.
- **Trabalho e entrega:** Adicionar wait_for/pós-condições; provar navegação em VS Code/Discord e configuração na fixture.
- **Aceite:** Mudança observada confirma sucesso; perda de janela/login/árvore incompleta não produz falso sucesso.
- **Execução:** status: parcial; wait_for nativo com polling bounded e timeout explícito foi implementado, e ações agora restringem a resolução ao app/janela do snapshot; enumeração real Linux foi observada, mas VS Code/Discord e pós-condição de uma ação em app externo não foram provados nesta sessão.

### T029 — Recuperar falhas e impedir loops

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T020,T028.
- **Trabalho e entrega:** Tratar stale refs, timeout, reobservação, estagnação e retry somente quando seguro.
- **Aceite:** Três ciclos sem progresso encerram/pedem ajuda; efeitos incertos não são repetidos cegamente.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; stale/timeout/cancel, erro estruturado, contador de estagnação de três ciclos e journal que bloqueia replay de efeitos `pending/unknown` existem no core/UI/store; reconciliação externa e retry seguro ainda faltam.

### T030 — Implementar shell controlado

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Ferramentas.
- **Depende de:** T014,T020.
- **Trabalho e entrega:** Executar argv ou shell explícito com cwd, env mínimo, timeout, streams, limite de saída e grupos de processos.
- **Aceite:** Cancelar encerra árvore controlada; saída grande não bloqueia IPC; código de saída e efeitos são registrados.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; argv sem shell, ambiente mínimo, cwd limitado, timeout, cancelamento, grupo privado e drenagem de saída passam; shell explícito continua classificado como arbitrário e exige aprovação.

### T031 — Implementar leitura e busca de arquivos

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Ferramentas.
- **Depende de:** T014.
- **Trabalho e entrega:** Listar/buscar/ler com limites, encoding, arquivos binários, timezone e metadados de data.
- **Aceite:** J03 retorna candidatos corretos no fixture; não confunde mtime com data comprovada de download.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; evidência: [T030–T040 — ferramentas](T030-T040-ferramentas-ui.md) e testes de UTF-8/binário, listagem, busca e limites.

### T032 — Implementar escrita e organização

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Ferramentas.
- **Depende de:** T031.
- **Trabalho e entrega:** Criar/mover/escrever/trash com atomicidade possível, colisões, symlinks, preview e reversão limitada.
- **Aceite:** J05 preserva conteúdo; overwrite sem autorização falha; traversal e troca de symlink são testados.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; evidência: [T030–T040 — ferramentas](T030-T040-ferramentas-ui.md), rename atômico, overwrite explícito, traversal e symlink testados.

### T033 — Abrir aplicativos e caminhos

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T014,T024.
- **Trabalho e entrega:** Resolver apps instalados, lançar com argumentos seguros e abrir pasta/arquivo no SO.
- **Aceite:** Abrir Discord/VS Code não depende de caminho fixo; app ausente recebe orientação; spawn não basta como sucesso.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; `apps.resolve` consulta um catálogo bounded no PATH sem spawn, o planejador reconhece aliases de Discord/VS Code/Chrome como `apps.launch` externo e launch/open usam argv com `observed:false` quando só houve spawn; prova de Discord/VS Code e catálogo por SO ainda abertas.

### T034 — Gerenciar processos

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T030.
- **Trabalho e entrega:** Listar e inspecionar processos; terminar com identidade validada e política de risco.
- **Aceite:** Não confunde PID reutilizado; processo próprio é cancelável; processo alheio respeita autorização.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; listagem por PID+comando, término com identidade, recusa de start-time divergente e observação bounded de ausência foram implementados; PID reuse real, árvore externa e processo autorizado ainda precisam de matrix adversarial por SO.

### T035 — Controlar janelas e clipboard

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T024,T033.
- **Trabalho e entrega:** Enumerar/focar janelas e ler/escrever clipboard sob escopo com limites de tamanho/MIME.
- **Aceite:** Permissões e limitações Wayland reportadas; clipboard não é coletado continuamente.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; enumeração/foco semântico base e `clipboard.read`/`clipboard.write` bounded entram no broker com aprovação para escrita, backends `wl-paste`/`wl-copy`, `xclip`, `pbpaste`/`pbcopy` e PowerShell, cancelamento e fixtures locais; foco cross-compositor, MIME além de `text/plain` e prova em sessões reais ainda estão abertos.

### T036 — Integrar escolha de ferramentas

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Core.
- **Depende de:** T019,T029,T030,T032,T033,T034,T035.
- **Trabalho e entrega:** Definir descrições/prompts de produto e contexto de capabilities para escolher GUI, shell ou arquivos.
- **Aceite:** J02 combina ferramentas e verifica resultado; não exige GUI para criar cada arquivo.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; o core agora classifica janelas, arquivos, clipboard e escrita por intenção textual conservadora, enquanto o broker revalida schema, escopo e approval; catálogo completo, modelo comercial/local e verificação de jornada J02 ainda seguem abertos.

### T037 — Construir conversa e streaming nativos

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** UX.
- **Depende de:** T009,T013,T020.
- **Trabalho e entrega:** Implementar histórico, entrada, streaming, erro e resumo de ações com detalhes expansíveis.
- **Aceite:** Conversa continua responsiva durante tools; estados são compreensíveis sem jargão interno.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; evidência: [T030–T040 — ferramentas e UI](T030-T040-ferramentas-ui.md), reducer, histórico, streaming, draft e erros implementados; QA visual ainda pendente.

### T038 — Integrar Parar e aprovações à UI

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** UX.
- **Depende de:** T014,T023,T027,T037,T068.
- **Trabalho e entrega:** Exibir ação/alvo/consequência, aprovar/negar e Parar persistente.
- **Aceite:** G2 demonstrado: texto→ação semântica→verificação→resposta e cancelamento, sem Pi CLI.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; Parar, modo aprovação e authorize/deny via broker existem; jornada real write→aprovação→pós-condição ainda não foi executada na janela.

### T039 — Criar onboarding e configurações

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** UX.
- **Depende de:** T022,T037.
- **Trabalho e entrega:** Configurar provider/modelo, permissões, atalhos, histórico e diagnósticos simples.
- **Aceite:** Usuário novo chega à primeira tarefa; credencial/permissão inválida tem recuperação clara.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; preferências/diagnóstico básico, provider configurável e adaptador de keyring Linux/fixture existem; formulário de credencial, permissões multiplataforma e onboarding completo ainda não.

### T040 — Polir janela residente e acessibilidade

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** UX.
- **Depende de:** T037,T038.
- **Trabalho e entrega:** Implementar ocultar/reabrir/sair, foco preservado, monitores/DPI, teclado e semântica da própria UI.
- **Aceite:** Sem roubo de foco na automação; UI testada por teclado/leitor de tela; Sair encerra workers.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; modo compacto, minimize/restore, tema e controles nativos foram implementados; captura visual, DPI e screen reader real ainda pendentes.

### T041 — Validar alpha com usuários de teste

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Produto.
- **Depende de:** T021,T036,T039,T040,T069,T070,T071,T072,T074,T076,T077,T078,T079,T080.
- **Trabalho e entrega:** Executar J01–J06, registrar fricções, falhas e corrigir bloqueadores do fluxo.
- **Aceite:** G3 aprovado com modelo local e comercial; todas falhas têm evidência e prioridade.
- **Execução:** status: externo; o fluxo técnico e a matriz QA estão preparados, mas alpha exige usuários de teste, provider local/comercial e jornadas J01–J06 reais.

### T042 — Escolher transcrição de voz

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Voz.
- **Depende de:** T008.
- **Trabalho e entrega:** Comparar engines locais/remotas por pt-BR, latência, licença, privacidade, distribuição e hardware.
- **Aceite:** ADR A06 define implementação v1 e critérios mensuráveis; nenhuma dependência de LLM multimodal.
- **Execução:** responsável: Codex; data: 16/09/2026; status: documental parcial; [VOICE-RFC](../docs/VOICE-RFC.md) fixa critérios e mantém escolha aberta sem simular benchmark. O contrato de estados foi implementado em `packages/agent-core/src/voice.ts`, mas a escolha de engine continua externa.

### T043 — Implementar captura push-to-talk

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** Voz.
- **Depende de:** T040,T042.
- **Trabalho e entrega:** Capturar áudio, dispositivos, nível, key down/up, duração máxima e revogação de permissão.
- **Aceite:** Key-up perdido, suspensão e cancelamento encerram microfone; fallback pela janela funciona.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial determinístico; `crates/audio-capture` implementa captura PCM bounded por PipeWire/ALSA no Linux, key-down/key-up, cancelamento por perda de foco, watchdog de duração, limite de bytes e grupo privado, com fixtures em `crates/audio-capture/src/lib.rs`. A UI mantém texto como caminho completo e voz desabilitada de forma factual até STT, seleção/permissão de dispositivo e integração do turno.

### T044 — Integrar STT e turno textual

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** Voz.
- **Depende de:** T020,T043.
- **Trabalho e entrega:** Criar adapter Transcriber, download verificado se necessário e envio de texto ao core.
- **Aceite:** Áudio vazio/erro não dispara tarefa; transcrição pode ser editada; engine cancela corretamente.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial determinístico; `VoiceController` implementa adapter, cancelamento, duração máxima, áudio/transcrição vazios e revisão antes de aceitar o texto, com testes em `packages/agent-core/test/voice.test.ts`. Engine STT, captura de áudio e envio efetivo ao core ainda são externos.

### T045 — Tratar atalhos e foco de voz por SO

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Plataforma.
- **Depende de:** T044,T005,T006,T007.
- **Trabalho e entrega:** Integrar atalhos suportados, conflitos e fallback sem atrapalhar entrada em outros apps.
- **Aceite:** Pressionar/soltar funciona em ambientes suportados; ausência de atalho é informada, não simulada.
- **Execução:** status: externo; depende de atalhos, foco e permissões em cada SO; não foi simulada uma captura global.

### T046 — Avaliar voz em português

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** QA.
- **Depende de:** T044,T045.
- **Trabalho e entrega:** Executar corpus de 50 frases com nomes/apps, ruído, dispositivos e intenções ambíguas.
- **Aceite:** J07 aprovada; métricas e falhas documentadas; mesmas aprovações de texto aplicadas.
- **Execução:** status: externo; corpus pt-BR, hardware/microfone e métricas ainda não foram executados; critérios estão em [VOICE-RFC](../docs/VOICE-RFC.md).

### T047 — Consolidar suporte Linux

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Plataforma.
- **Depende de:** T035,T040,T045.
- **Trabalho e entrega:** Corrigir diferenças GNOME/KDE Wayland e X11; documentar setup, portais e limitações.
- **Aceite:** Jornadas aplicáveis e residência verificadas em cada sessão real; sem exigir XWayland como substituto.
- **Execução:** status: parcial; Ubuntu/GNOME/Wayland real foi observado; X11/KDE, clipboard, foco e pacote instalado continuam na [matriz de plataforma](../docs/PLATFORM-MATRIX.md).

### T048 — Consolidar suporte Windows

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Plataforma.
- **Depende de:** T035,T040,T045.
- **Trabalho e entrega:** Integrar paths, shell, lifecycle, acessibilidade, áudio e diferenças de integridade.
- **Aceite:** Jornadas em máquina real com usuário normal; falhas por elevação não são reportadas como sucesso.
- **Execução:** status: externo; workflow comprova somente compilação/testes de código; UIA, fixture, foco, áudio, permissões e pacote Windows continuam sem máquina de aceite.

### T049 — Consolidar suporte macOS

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Plataforma.
- **Depende de:** T035,T040,T045.
- **Trabalho e entrega:** Integrar bundle, permissões, ciclo de vida, foco, áudio e caminhos.
- **Aceite:** Jornadas com app empacotado; permissões revogadas e reinstalação têm fluxo claro.
- **Execução:** status: externo; workflow comprova somente compilação/testes de código; AXUIElement, bundle, permissões, foco, áudio e reinstalação continuam sem máquina de aceite.

### T050 — Executar suíte adversarial

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Segurança.
- **Depende de:** T022,T029,T032,T038.
- **Trabalho e entrega:** Testar injeção, exfiltração, argumentos, replay, symlinks, segredos e contorno entre ferramentas.
- **Aceite:** Todos cenários de política passam; zero achado crítico aberto antes de beta.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; A01–A10 agora têm testes nomeados em política, broker, runtime, supervisor e session-store; fault injection no processo empacotado e jornada real de efeito ainda faltam para fechar o aceite global.

### T051 — Avaliar providers e modelos

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** QA.
- **Depende de:** T019,T021,T036.
- **Trabalho e entrega:** Rodar benchmark textual com provider local/comercial, troca de sessão e erros de rede/contexto.
- **Aceite:** J01 e R02/R03 têm evidência; payload multimodal proibido no teste; modelos incompatíveis identificados.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; fake e OpenAI-compatible/SSE local passam; [QA](../docs/QA.md) define o registro comparável, mas conta comercial, modelo local e métricas de qualidade/custo não foram simulados.

### T052 — Automatizar jornadas E2E

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** QA.
- **Depende de:** T028,T036,T038,T047,T048,T049,T051.
- **Trabalho e entrega:** Criar fixture/replay de estado e rodar J02–J06 em desktop real, incluindo apps externos controlados.
- **Aceite:** Metas por plataforma/modelo atingidas; resultados reproduzíveis e sem falso sucesso.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; `crates/supervisor/tests/e2e.rs` atravessa core TypeScript, supervisor NDJSON, broker/policy, clipboard fixture e approval em duas jornadas; J02–J08 em desktop real por plataforma ainda não foram executados.

### T053 — Medir desempenho e consumo

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** QA.
- **Depende de:** T046,T047,T048,T049.
- **Trabalho e entrega:** Medir startup, UI, memória, CPU idle, snapshots, STT, tokens e custo com hardware registrado.
- **Aceite:** Metas medidas e gargalos corrigidos; modelo local separado do overhead do app.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; `pnpm benchmark:core` mede startup, turno fake e RSS p50/p95 do processo privado no hardware atual, além do spike T002; UI, CPU idle, STT, modelos, custo e matriz entre plataformas dependem de hardware e sessões publicadas.

### T054 — Testar falhas e recuperação

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** QA.
- **Depende de:** T021,T023,T030,T044.
- **Trabalho e entrega:** Injetar crash, stream quebrado, worker preso, suspensão, perda de foco e cancelamento em todos estados.
- **Aceite:** J08 e recuperação passam; nenhum novo efeito após cancelamento e nenhum replay automático após crash.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; cancelamento do core, EOF do supervisor, grupo privado de processos, recovery SQLite, journal `pending`→`unknown`, corrupção diagnosticável, timeout, saída limitada e A10 de não-replay passam; crash/worker preso no pacote e J08 real seguem em [QA](../docs/QA.md).

### T055 — Criar artefatos e instaladores

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** Entrega.
- **Depende de:** T015,T047,T048,T049.
- **Trabalho e entrega:** Empacotar runtime/core/UI/deps; definir formatos, assinatura, SBOM e hashes por plataforma.
- **Aceite:** Instalação limpa não precisa de Node/Pi; permissões do pacote final funcionam.
- **Execução:** status: parcial forte; há apenas [bundle de desenvolvimento](../docs/RELEASE.md), com Node embutido, dependências copiadas, manifesto SHA-256, SBOM dev CycloneDX 1.5 e smoke reproduzível de instalação/troca/rollback/desinstalação em Linux, ainda sem instalador final, assinatura, notices/SBOM de release ou atualização entre versões reais.

### T056 — Validar instalação e atualização

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Entrega.
- **Depende de:** T021,T055.
- **Trabalho e entrega:** Testar instalar, atualizar, migrar, rollback compatível e desinstalar com preservação/remoção de dados.
- **Aceite:** Matriz limpa aprovada em cada SO; artefato corrompido não é aceito.
- **Execução:** status: parcial; `scripts/validate-bundle.sh` valida manifesto/SBOM e cobre instalação temporária, preservação durante upgrade, rollback de release e escolha explícita de remoção de dados no bundle Linux; matriz limpa por SO, migração entre versões reais e instalador final continuam externos.

### T057 — Executar beta de uso diário

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** Produto.
- **Depende de:** T041,T046,T050,T052,T053,T054,T056,T075,T081,T082.
- **Trabalho e entrega:** Rodar dogfooding de cinco dias por SO, consolidar bugs e corrigir bloqueadores.
- **Aceite:** G5 aprovado: estabilidade e critérios medidos; sem incidentes críticos ou tarefas silenciosamente perdidas.
- **Execução:** status: externo; exige cinco dias de uso por SO, portanto não é simulada por testes locais.

### T058 — Preparar documentação e release

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Entrega.
- **Depende de:** T057.
- **Trabalho e entrega:** Escrever onboarding, permissões, modelos, limites, voz, troubleshooting, contribuição, licenças e changelog.
- **Aceite:** Usuário instala e completa jornada seguindo docs; compatibilidade e limitações estão explícitas.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; [DEVELOPMENT](../docs/DEVELOPMENT.md), [RELEASE](../docs/RELEASE.md), [QA](../docs/QA.md), [CHANGELOG](../CHANGELOG.md), dependências e limitações foram escritos; assinatura e instaladores finais dependem de T055–T057.

### T059 — Definir manutenção e suporte

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Entrega.
- **Depende de:** T015,T058.
- **Trabalho e entrega:** Definir triagem, reprodução redigida, resposta a vulnerabilidades, atualização do fork e calendário de revisão.
- **Aceite:** Responsável e processo documentados; pacote de diagnóstico não contém dados sensíveis por padrão.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; [SUPPORT](../docs/SUPPORT.md) define triagem redigida, vulnerabilidades e upstream; canal/responsável operacional de produção ainda precisa ser escolhido.

### T060 — Auditar conclusão e publicar v1

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Produto.
- **Depende de:** T058,T059.
- **Trabalho e entrega:** Revisar R01–R11, evidências/gates, artefatos finais e canal; executar smoke final e release.
- **Aceite:** G6 aprovado; versão instalável distribuída no canal escolhido com hashes, docs e pendências não bloqueantes registradas.
- **Execução:** status: não iniciada para publicação; [RELEASE](../docs/RELEASE.md) contém o gate, mas G6 continua aberto por multiplataforma, pacote, voz e beta.

### T061 — Avaliar OCR e visão opcionais

- [ ] Concluída
- **Prioridade / esforço:** P2 / G. **Responsabilidade:** Desktop.
- **Depende de:** T060.
- **Trabalho e entrega:** Estudar lacunas semânticas e pipeline que converta visão em observações textuais.
- **Aceite:** RFC separada com privacidade/custo; modelos textuais continuam plenamente suportados no escopo base.
- **Execução:** status: RFC registrada em [OPTIONAL-CAPABILITIES](../docs/OPTIONAL-CAPABILITIES.md); não é dependência da fatia textual.

### T062 — Avaliar TTS e wake word

- [ ] Concluída
- **Prioridade / esforço:** P2 / M. **Responsabilidade:** Voz.
- **Depende de:** T060.
- **Trabalho e entrega:** Projetar resposta falada e ativação opcional com consentimento e limites de captura.
- **Aceite:** RFC e benchmark; captura contínua nunca habilitada implicitamente.
- **Execução:** status: RFC registrada em [OPTIONAL-CAPABILITIES](../docs/OPTIONAL-CAPABILITIES.md) e [VOICE-RFC](../docs/VOICE-RFC.md); benchmark e implementação ficam fora desta execução.

### T063 — Avaliar extensibilidade e automações

- [ ] Concluída
- **Prioridade / esforço:** P2 / G. **Responsabilidade:** Arquitetura.
- **Depende de:** T060.
- **Trabalho e entrega:** Estudar plugins/MCP, tarefas agendadas, isolamento e permissões antes de abrir execução externa.
- **Aceite:** RFC com modelo de confiança e revogação; nenhum plugin arbitrário incluído na v1 por conveniência.
- **Execução:** status: RFC registrada em [OPTIONAL-CAPABILITIES](../docs/OPTIONAL-CAPABILITIES.md); plugins arbitrários não entram na v1.

### T064 — Avaliar memória e sincronização

- [ ] Concluída
- **Prioridade / esforço:** P2 / G. **Responsabilidade:** Produto.
- **Depende de:** T060.
- **Trabalho e entrega:** Estudar memória de preferências, exportação e sync com controles de privacidade.
- **Aceite:** RFC define retenção, exclusão, conflitos e segurança; sem coleta automática adicional na v1.
- **Execução:** status: RFC registrada em [OPTIONAL-CAPABILITIES](../docs/OPTIONAL-CAPABILITIES.md); retenção/sync não são habilitados implicitamente.


## Pacotes adicionais — detalhamento de interface e implementação

Estes pacotes complementam T001–T064. Não são opcionais nem pressupõem que tarefas originais já estejam concluídas. A numeração é estável; executar pelas dependências, não em ordem numérica. Estimativas sobrepostas às tarefas originais não devem ser somadas duas vezes: ao estimar sprint, contabilizar o pacote detalhado e apenas o restante do pacote original.

### T065 — Especificar e prototipar a janela flutuante

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** UX. **Fase:** F1.
- **Depende de:** T001.
- **Trabalho e entrega:** Reproduzir a especificação visual em protótipo nativo com dados sintéticos; testar proporções antes de integrar serviços.
- **Referência:** [10-interface-janela-flutuante.md](10-interface-janela-flutuante.md).
- **Subtarefas:**
  - [ ] Desenhar invocação, conversa, compacto, aprovação, voz e erro.
  - [ ] Materializar cabeçalho, campo, faixa de ação e seletor de modelo em egui.
  - [ ] Registrar ajustes de medidas e alternativas de decoração por SO.
- **Aceite:** Capturas reais demonstram hierarquia e controles legíveis em 440 pt; nenhum CLI, dashboard ou WebView substitui a janela.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; modos, medidas 440/360, controles e limites foram materializados no `apps/desktop`; capturas reais e ajuste por SO ainda faltam.

### T066 — Implementar controlador da janela

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop. **Fase:** F2.
- **Depende de:** T009,T065.
- **Trabalho e entrega:** Criar modos visuais e lifecycle de janela conforme capabilities do SO.
- **Referência:** [10-interface-janela-flutuante.md](10-interface-janela-flutuante.md).
- **Subtarefas:**
  - [ ] Implementar invocar, crescer uma vez, redimensionar, arrastar e restaurar geometria.
  - [ ] Detectar monitor/DPI, retirar coordenadas inválidas e respeitar área útil.
  - [ ] Implementar Manter acima opt-in, decoração nativa fallback e regras de foco.
- **Aceite:** Monitor desconectado não perde janela; primeiro envio não causa crescimento por token; limitações do compositor têm fallback.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; viewport invocation/conversation/compact/approval, minimizar, nível always-on-top e fechamento ordenado existem; monitor/DPI/drag/restore real continuam pendentes.

### T067 — Implementar tokens e componentes nativos

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** UX. **Fase:** F2.
- **Depende de:** T009,T065.
- **Trabalho e entrega:** Centralizar tema, fontes, escala, cores e controles reutilizáveis.
- **Referência:** [10-interface-janela-flutuante.md](10-interface-janela-flutuante.md).
- **Subtarefas:**
  - [ ] Implementar claro/escuro/sistema e texto escalável.
  - [ ] Criar controles com default/hover/foco/pressionado/disabled/loading/error/success conforme semântica.
  - [ ] Medir contraste dos pares efetivos; documentar licenças de fontes/ícones e fallback.
- **Aceite:** Tema consistente sem cores avulsas; foco imediato e geometria estável; texto e controles atingem contraste planejado.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; tema claro/escuro, escala básica, foco/disabled e controles egui estão em `apps/desktop`; contraste formal e leitor de tela real ainda pertencem a T074.

### T068 — Implementar compositor e histórico de conversa

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** UX. **Fase:** F2.
- **Depende de:** T037,T066,T067.
- **Trabalho e entrega:** Implementar mensagens, Markdown restrito, campo multilinha, streaming e scroll.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Tratar Enter, Shift+Enter, IME, duplo envio e rascunho persistido.
  - [ ] Preservar leitura durante streaming e oferecer Novas mensagens.
  - [ ] Suportar código longo/caminhos sem expandir janela e bloquear envio durante run ativo.
- **Aceite:** UI01–UI03 passam; rascunho não some em erro de conexão; texto não executa HTML/comando.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; streaming, draft multilinha, Enter/Shift+Enter, scroll e bloqueio de envio durante run estão em `apps/desktop`; IME e screenshots reais seguem em T075.

### T069 — Implementar modo compacto e visibilidade

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Desktop. **Fase:** F3.
- **Depende de:** T023,T038,T066.
- **Trabalho e entrega:** Recolher acompanhamento mantendo controle e vínculo à conversa.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Exibir ação/alvo, Expandir e Parar no compacto.
  - [ ] Ocultar durante run preserva parada; ocultar idle preserva rascunho.
  - [ ] Aprovação/conclusão/erro no compacto não roubam foco nem desaparecem automaticamente.
- **Aceite:** UI04/UI14 passam; compactar não reinicia run; nenhum caminho visual deixa efeitos sem controle de parada disponível.
- **Execução:** responsável: Codex; data: 16/09/2026; status: concluída no recorte local; compacto preserva run/draft e mantém Expandir/Parar; aprovação/erro em app empacotado ainda precisam de QA.

### T070 — Implementar navegação de sessões

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** UX. **Fase:** F3.
- **Depende de:** T021,T068.
- **Trabalho e entrega:** Criar Histórico, Nova conversa e operações de sessão na mesma janela.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Paginar/buscar por recência e mostrar estado factual.
  - [ ] Retornar à conversa restaurando rascunho e posição.
  - [ ] Exportar/apagar com política de dados e bloqueio de exclusão de run ativo.
- **Aceite:** Trocar sessão não reproduz tools; não existe sidebar permanente; conversas longas não congelam render.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; histórico persistido tem busca por título, paginação, estado do último run, troca/restauração de mensagens, exportação redigida e exclusão confirmada; posição/rascunho por sessão e teste de conversa longa ainda faltam.

### T071 — Implementar preferências e onboarding completos

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** UX. **Fase:** F3.
- **Depende de:** T039,T067.
- **Trabalho e entrega:** Construir configuração progressiva com conexão, permissões e aparência.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Validar credenciais com campos preservados em falha.
  - [ ] Mostrar capacidades ausentes e caminho de recuperação.
  - [ ] Bloquear troca de provider durante run; aplicar tema/escala sem reiniciar.
- **Aceite:** UI10 passa e usuário chega à primeira tarefa sem conhecer Pi/xa11y; negar voz não bloqueia texto.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; tema e "manter acima" são preferências não secretas persistidas, a UI informa provider/capabilities/recuperação e oferece conta + campo mascarado para gravar explicitamente no keyring; endpoint/modelo também podem ser salvos sem segredo e são injetados apenas no próximo core. Reinício imediato, permissões multiplataforma e onboarding completo ainda faltam.

### T072 — Implementar atividade, aprovação e falha parcial

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** UX. **Fase:** F3.
- **Depende de:** T038,T068.
- **Trabalho e entrega:** Renderizar eventos reais, previews e decisões contextuais sem excesso de painéis.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Criar faixa de ação e lista de atividade expansível.
  - [ ] Vincular decisão a approval_id; impedir Enter residual e confirmar revalidação.
  - [ ] Mostrar cancelamento, sucesso verificado e efeito parcial/incerto distintamente.
- **Aceite:** UI05/UI06/UI12 passam; Parar permanece acessível; nenhum percentual é inventado; resultado parcial não aparece como sucesso integral.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; faixa de atividade, preview de tool/argumentos, approval_id persistido, aprovação/negação, erro, unknown distinto de sucesso e Parar estão ligados ao broker; pós-condição de app real e lista de atividade visual completa ainda faltam.

### T073 — Implementar experiência visual e acessível de voz

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Voz. **Fase:** F4.
- **Depende de:** T044,T068.
- **Trabalho e entrega:** Apresentar captura, transcrição, revisão e falhas na mesma janela.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Exibir duração/nível reais e controles de gravação por teclado.
  - [ ] Implementar revisão padrão, preferência de envio direto e cancelamento sem turno.
  - [ ] Tratar dispositivo removido, silêncio e áudio excedente com microcopy específica.
- **Aceite:** UI09 passa; microfone fica inativo após encerramento; usuário consegue usar voz sem manter tecla pressionada.
- **Execução:** status: parcial determinístico; a janela mantém texto como caminho completo e indica voz não configurada; o contrato de revisão/cancelamento existe no core e é testado, mas captura/STT/dispositivo/permissões e a UI nativa de voz não foram implementados nem simulados.

### T074 — Validar foco, teclado, escala e acessibilidade

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** QA. **Fase:** F3.
- **Depende de:** T068,T069,T071,T072.
- **Trabalho e entrega:** Verificar janela por teclado, leitores de tela e escalas reais.
- **Referência:** [10-interface-janela-flutuante.md](10-interface-janela-flutuante.md).
- **Subtarefas:**
  - [ ] Executar UI07/UI08/UI13 em plataformas de referência.
  - [ ] Testar largura 320/360/440/640 e texto 100/150/200%.
  - [ ] Verificar labels, anúncio de mudanças sem leitura token a token, menus e retorno de foco.
- **Aceite:** Nenhum controle crítico cortado/inacessível; nenhuma funcionalidade só por hover; evidência por SO e falhas corrigidas.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; teclado, labels visuais, modos e escalas de viewport têm cobertura de código; leitor de tela, 100/150/200%, 320/640 e evidência por SO aguardam QA real.

### T075 — Criar suíte de estados e fluxos da interface

- [ ] Concluída
- **Prioridade / esforço:** P1 / G. **Responsabilidade:** QA. **Fase:** F5.
- **Depende de:** T068,T070,T072,T073,T074.
- **Trabalho e entrega:** Cobrir UI01–UI16 com eventos sintéticos e smoke no desktop real.
- **Referência:** [11-fluxos-e-estados-da-interface.md](11-fluxos-e-estados-da-interface.md).
- **Subtarefas:**
  - [ ] Criar fixtures para estado vazio, aprovação, timeout, conexão perdida e erro parcial.
  - [ ] Registrar screenshots reais claro/escuro de todos os modos.
  - [ ] Testar teclado/scroll/IME e aprovações além da comparação visual.
- **Aceite:** Relatório distingue testes automáticos e manuais; nenhuma captura bonita substitui aceite comportamental.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; reducer e fixtures de erro/approval/compacto têm testes unitários; UI01–UI16 com screenshots e teclado/IME ainda não foram executados no desktop real.

### T076 — Polir responsividade e consumo da janela

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Desktop. **Fase:** F3.
- **Depende de:** T040,T074.
- **Trabalho e entrega:** Medir custo de render, listas longas e mudanças de estado.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Agrupar deltas e virtualizar histórico quando necessário.
  - [ ] Solicitar repaint por evento; eliminar loops de animação ociosos.
  - [ ] Medir abrir/recolher/expandir e prioridade de Parar.
- **Aceite:** UI16 passa; nenhum bloqueio de rede/SO na thread de render; metas de responsividade possuem medição.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; resize por mudança de modo evita crescimento por token e repaint é bounded; benchmark de consumo/render e histórico grande ainda falta.

### T077 — Implementar reducer e contratos de apresentação

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Arquitetura. **Fase:** F3.
- **Depende de:** T010,T020,T068.
- **Trabalho e entrega:** Separar ViewModel, UiCommand e eventos de execução.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Deduplicar eventos, correlacionar run e preservar terminalidade.
  - [ ] Tratar eventos antigos/fora de ordem, overflow e queda do core.
  - [ ] Definir reconexão sem iniciar tool; testar reducer determinístico.
- **Aceite:** UI11 passa; eventos visuais não concedem autorização nem mudam resultado terminal de outro run.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte no recorte local; `PresentationState` deduplica completion, preserva run compacto, representa erro/cancelamento e agora rejeita eventos de run antigo por `run_id`, com fixtures de delta atrasado, terminalidade e falha posterior. Screenshots, IME, reconexão visual e QA real continuam em T075.

### T078 — Detalhar e testar dados e migrações

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Core. **Fase:** F3.
- **Depende de:** T021,T022.
- **Trabalho e entrega:** Consolidar esquema, checkpoints, índices, retenção e exportação.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Documentar tabelas/limites e dono único de escrita.
  - [ ] Testar migração N−1→N, rollback de falha e recuperação de unknown.
  - [ ] Testar exclusão/exportação redigida e não persistência de áudio/aprovação reutilizável.
- **Aceite:** Crash entre efeito e resultado não provoca replay; migrações preservam mensagens e falham de forma recuperável.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; SQLite v1→v2, reopen, preferences, recovery, retenção, export redacted, approvals auditáveis/invalidados e idempotência têm testes; rollback interrompido e prova negativa de áudio/approval reutilizável no pacote ainda faltam.

### T079 — Construir fixture semântica adversarial

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** Desktop. **Fase:** F3.
- **Depende de:** T025,T027.
- **Trabalho e entrega:** Criar aplicação de teste controlada que exerça bordas da acessibilidade.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Incluir nomes duplicados, senha, lista virtualizada, diálogo e nó recriado.
  - [ ] Injetar mudança de janela/foco entre observação e ação.
  - [ ] Medir limites de snapshot e retorno de ação sem pós-condição.
- **Aceite:** Referência antiga nunca aciona outro alvo; senha não vaza; dispatch sem resultado não conta como sucesso.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial; `MockDesktop` cobre duplicidade, senha, toggle, stale ref e pós-condição local; fixture nativa com diálogo/lista virtualizada/nó recriado ainda não está disponível.

### T080 — Consolidar matriz de efeitos das ferramentas

- [ ] Concluída
- **Prioridade / esforço:** P0 / M. **Responsabilidade:** Ferramentas. **Fase:** F3.
- **Depende de:** T030,T032,T034.
- **Trabalho e entrega:** Especificar pré/pós-condição, risco, timeout e repetição de cada ferramenta.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Testar arquivo existente, caminho malicioso e movimento cross-device parcial.
  - [ ] Testar argv, saída excessiva, PID reutilizado e processo órfão.
  - [ ] Registrar quais ações permitem rollback e quais exigem reconciliação.
- **Aceite:** Toda tool com efeito tem estratégia de verificação/cancelamento; nenhum retry cego de unknown.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; [TOOL-MATRIX](../docs/TOOL-MATRIX.md) cobre todas as tools atuais, incluindo query/wait_for, ambiente mínimo, grupos privados, start-time de PID e fallback cross-device; reconciliação externa e matriz por SO ainda faltam.

### T081 — Validar falhas entre UI, core e workers

- [ ] Concluída
- **Prioridade / esforço:** P0 / G. **Responsabilidade:** QA. **Fase:** F5.
- **Depende de:** T054,T077,T078.
- **Trabalho e entrega:** Executar fault injection por fronteira com UI final.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Matar core/worker em momentos distintos e testar crash loop.
  - [ ] Verificar microfone desligado, Parar prioritário e fechamento ordenado.
  - [ ] Confirmar histórico visível e incerteza após reconexão.
- **Aceite:** UI15 passa; nenhum crash vira completed e nenhum processo próprio órfão fica ativo sem diagnóstico.
- **Execução:** responsável: Codex; data: 16/09/2026; status: parcial forte; cancelamento core, EOF inesperado, correlação, recovery, journal `pending`→`unknown`, E2E broker/core, A10 de não-replay, grupo de processos, reducer e rejeição de eventos stale por `run_id` têm testes; fault injection do worker/UI empacotado e crash-loop real aguardam T055/T082.

### T082 — Validar experiência no pacote instalado

- [ ] Concluída
- **Prioridade / esforço:** P1 / M. **Responsabilidade:** Entrega. **Fase:** F5.
- **Depende de:** T055,T056,T075,T081.
- **Trabalho e entrega:** Executar onboarding e fluxos com instalador final fora do ambiente de desenvolvimento.
- **Referência:** [12-detalhamento-tecnico-e-entregaveis.md](12-detalhamento-tecnico-e-entregaveis.md).
- **Subtarefas:**
  - [ ] Testar permissões, keyring, fontes, atalhos, geometria e residência.
  - [ ] Repetir modos/voz/parada em cada SO e ambiente Wayland publicado.
  - [ ] Registrar evidências do pacote e atualizar matriz de limitações.
- **Aceite:** Janela flutuante e fluxos funcionam sem Node/Pi instalados; diferenças do pacote final são corrigidas antes da beta.
- **Execução:** status: parcial; [RELEASE](../docs/RELEASE.md) define o smoke obrigatório, e `scripts/validate-bundle.sh` passou no bundle Linux com Node, dependências, manifesto SHA-256 e SBOM dev; isso não substitui instalador final nem aceite fora da árvore de desenvolvimento.
