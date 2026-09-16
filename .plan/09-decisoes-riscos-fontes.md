# Decisões, riscos e fontes

## Decisões

| ID | Estado | Decisão / questão | Fechamento |
|---|---|---|---|
| A00 | Requisito definido | Produto próprio, Rust/egui, TS derivado de fork Pi, xa11y sem alteração inicial | Contexto original |
| A01 | Proposta | Core próprio em runtime JS supervisionado vs embedding | T002/T008, medir ciclo de vida e distribuição |
| A02 | Proposta | IPC privado JSON-RPC/NDJSON e schemas compartilhados | T010/T011 |
| A03 | Proposta | Efeitos centralizados em broker Rust; sem ferramentas diretas herdadas | T014/T016 |
| A04 | Proposta | SQLite local + keyring; retenção configurável | T021/T022 |
| A05 | Pendente | Provider/modelo comercial e local de referência, protocolo fallback | T001/T017–T019 |
| A06 | Pendente | STT local/remoto, engines, licença e distribuição de pesos | T042 |
| A07 | Pendente | Versões mínimas SO, arquiteturas e formatos de instalador | T008/T055 |
| A08 | Proposta | Uma execução com efeitos por desktop, sem plugins arbitrários na v1 | T020/T050 |
| A09 | Pendente | Nome final, licença do produto e canal de releases | T001/T003/T058 |

Fechar uma decisão exige contexto, alternativas, evidência, escolha e consequências em `docs/architecture/adr-XXXX.md` durante implementação. Não preencher versões/SHA por memória: fixá-los nos spikes e nos lockfiles.

## Riscos

| Risco | Sinal / impacto | Mitigação e responsável técnico |
|---|---|---|
| Árvore incompleta em Discord/VS Code | Não encontrar alvo sem visão | T004/T028; responsável desktop; relatar limitação e assistência manual |
| Wayland restringe foco/atalho | Fluxo não equivalente aos outros SOs | T005/T047; responsável plataforma; probes e fallbacks explícitos |
| Core Pi acoplado ao CLI | Fork cresce ou exige produto upstream | T003/T012; responsável core; extração mínima e teste sem CLI |
| APIs de acessibilidade bloqueiam | Parada não funciona | T004/T023/T054; worker isolado e supervisor |
| Modelo textual emite ações inválidas | Loop ou execução errada | T019/T029; validação, limites e avaliações |
| Prompt injection/exfiltração | Efeito fora da intenção | T014/T050; broker e escopo externo ao modelo |
| Perda de evento/crash | Repetir operação irreversível | T021/T054; journal, resultado unknown e reconciliação |
| Embalagem runtime/xa11y | Funciona só em dev | T002/T055/T056; instalação limpa cedo |
| Permissões variam com versão/assinatura | App não acessa desktop após instalar | T006/T007/T048/T049; teste do pacote final |
| STT erra nome/alvo | Executar tarefa errada | T042/T046; edição, esclarecimento e mesma política de texto |
| Escopo “universal” sem limite | Projeto nunca termina | Requisitos v1, matriz de suporte e gates objetivos |
| Manutenção de fork e dependências | Divergência/vulnerabilidades | T015/T059; pins, revisão e atualização controlada |

## Fontes consultadas em 16/09/2026

O plano técnico é uma proposta própria. As páginas abaixo confirmam apenas bases e possibilidades; não substituem spikes no commit/versão escolhido.

- [Contexto original](00-contexto-original.txt): requisitos de produto fornecidos pelo usuário.
- [Pi — site oficial](https://pi.dev/): apresenta suporte a providers, streaming/conversação e integração programática. Essas capacidades motivam a auditoria de reaproveitamento, não provam desacoplamento do CLI.
- [Pi — upstream resolvido](https://github.com/earendil-works/pi): o endereço histórico `https://github.com/badlogic/pi-mono` redirecionou para este repositório na consulta. Confirmar origem e SHA em T003; não assumir nomes históricos de pacotes como definitivos.
- [xa11y — repositório oficial](https://github.com/xa11y/xa11y): documenta API Rust, ações semânticas e backends AT-SPI2, UI Automation e AXUIElement. Isso sustenta o adapter proposto; cobertura real depende do aplicativo e ambiente. Permissões devem ser verificadas na versão escolhida, inclusive diferenças de macOS.

Os demais detalhes (runtime, schema, persistência, metas, política e estrutura de módulos) são escolhas propostas do Vox. Consultar documentação oficial de egui/eframe, portais, APIs nativas e engines de voz ao fixar dependências em T002–T008/T042; nenhum suporte específico dessas bibliotecas foi considerado validado nesta entrega.

## Decisões de interface desta revisão

| ID | Estado | Escolha | Validação |
|---|---|---|---|
| A10 | Requisito confirmado pelo usuário | Janela flutuante minimalista e moderna | UX01–UX10 |
| A11 | Proposta | Modos invocação/conversa/compacto; medidas do documento 10 | T065/T066 |
| A12 | Proposta | Neutros com azul discreto; tema do SO; fonte sans única + mono | T067, contraste e licença |
| A13 | Proposta | Manter acima opt-in; ocultar run conserva Parar | T066/T069, matriz por compositor |
| A14 | Proposta | Revisar voz por padrão; envio direto configurável | T073 |
| A15 | Proposta | Rascunho editável durante run; novo envio só após parada/conclusão | T068/T077 |

Nenhum token, tamanho ou comportamento específico de APIs de janela foi validado em runtime nesta revisão. O uso de Hallmark orienta contenção visual/estados; a stack continua nativa Rust/egui. Riscos adicionais: decoração customizada quebrar movimento/acessibilidade; janela compacta ocultar decisão importante; eventos fora de ordem exibirem resultado incorreto; tamanho pequeno não suportar escala de texto. Mitigações estão em T066/T069/T074/T077, com fallback nativo e critérios verificáveis.
