# T001 — Alinhamento de referências de produto

- **Status:** concluída
- **Responsável:** Codex, na execução solicitada pelo usuário
- **Data da revisão:** 16/09/2026
- **Escopo:** revisar o contexto oficial, R01–R11, J01–J08 e os gates iniciais; registrar referências, baseline de hardware, orçamento conhecido e decisões ainda abertas.

## Evidências de entrada

- [Contexto original](00-contexto-original.txt), que permanece como autoridade de produto.
- [Produto, requisitos e escopo](01-produto.md), incluindo R01–R11 e J01–J08.
- [Roadmap e recursos](06-roadmap.md), incluindo G0–G6 e os recursos necessários.
- [Decisões, riscos e fontes](09-decisoes-riscos-fontes.md), incluindo A00–A15 e os riscos já identificados.
- [Backlog executável](07-tarefas.md), especialmente T001 e suas dependências.

Esta é uma revisão de alinhamento documental. Ela não comprova ainda runtime, acessibilidade, providers, voz, builds ou suporte multiplataforma; essas provas permanecem nas tarefas posteriores.

## Identidade e referências fixadas

| Item | Alinhamento atual | Limite da decisão |
|---|---|---|
| Nome de trabalho | **Vox** | Nome inferido do diretório e não aprovado como marca final. A09 continua aberta. |
| Produto | Assistente de desktop independente, residente, conversacional, multimodelo e capaz de operar o computador dentro do escopo publicado. | Não é CLI, TUI, wrapper ou distribuição do Pi. “Universal” não promete controlar interfaces sem semântica suficiente. |
| Interface | Janela flutuante nativa, compacta, minimalista e moderna, com conversa como fluxo principal. | A geometria e os estados detalhados ainda precisam do protótipo T065. |
| Percepção primária | Acessibilidade semântica através de xa11y, complementada por shell, arquivos e APIs nativas quando forem a ferramenta adequada. | OCR/visão são opcionais pós-v1; nenhum modelo multimodal é requisito. |
| UI nativa | Rust com egui/eframe. | A escolha de runtime e a fronteira Rust/TypeScript serão medidas em T002/T008. |
| Base de agente | Fork seletivo de conceitos e componentes úteis do Pi/pi.dev para providers, conversa, streaming, ferramentas, sessões e cancelamento. | A origem, SHA, licença e módulos reutilizáveis serão fixados em T003; o CLI/TUI do Pi não será o produto. |
| Apps de referência | VS Code para J02/J06, Discord para J04 e uma fixture controlada para provas repetíveis de acessibilidade, autorização e recuperação. | Versões, contas de teste e ações exatas ainda dependem de T004/T008/T052. |

Não foi fornecido no contexto original um concorrente comercial ou uma marca de assistente a ser copiada. Portanto, nenhum foi inventado como referência de identidade; as referências acima são funcionais e técnicas.

## Providers e modelos

O requisito está alinhado para dois caminhos, ambos textuais:

1. pelo menos um provider comercial de referência, com streaming, autenticação, chamadas estruturadas e erros legíveis;
2. pelo menos um modelo local ou endpoint compatível, capaz de percorrer a mesma jornada sem provider comercial.

Nenhum provider, nome de modelo, endpoint, protocolo de fallback ou orçamento numérico foi escolhido nesta tarefa. Isso fica registrado como A05 e será decidido com evidência em T017–T019, depois do baseline de T002/T008. A transcrição de voz permanece separada do LLM e será escolhida em T042; áudio não será enviado ao modelo textual por antecipação.

## Baseline de hardware e ambiente observado

Levantamento somente leitura realizado no checkout em 16/09/2026:

| Item | Baseline registrado |
|---|---|
| Sistema de referência | Ubuntu 24.04, kernel `7.0.0-31-generic`, `x86_64` |
| Sessão gráfica | GNOME em Wayland |
| CPU | AMD Ryzen 5 3400G, 8 CPUs lógicas observadas |
| Memória | 13 GiB de RAM; 4 GiB de swap |
| GPUs | AMD Radeon RX 580 2048SP e AMD Radeon Vega integrada |
| Monitores | Dois monitores ativos: 1920×1080 e 1366×768 |
| Plataforma inicial de prova | Linux/Wayland nesta máquina |

Esse baseline habilita o trabalho local e a investigação de Wayland, mas não é evidência de suporte a Windows, macOS, KDE, X11, outros hardwares ou outras versões. Os ambientes adicionais, arquiteturas, versões mínimas e instaladores continuam em A07 e nas tarefas T005–T008/T047–T056.

## Orçamento e limites de gasto

- O usuário não forneceu um valor numérico de orçamento.
- Nenhum provider pago, crédito, certificado, conta de distribuição ou serviço externo foi utilizado ou contratado durante T001.
- Para os próximos spikes, a restrição operacional é manter o custo controlado, preferir fixture/modelo local quando possível e registrar qualquer custo de API antes de consolidar a escolha em A05.
- O orçamento de API, CI, assinatura/distribuição e eventual STT pago precisa ser quantificado antes de T008/T017/T042/T055. A ausência de valor não bloqueia as escolhas reversíveis de arquitetura, mas impede declarar uma escolha comercial final.

## Revisão de requisitos e jornadas

### Requisitos R01–R11

| ID | Resultado da revisão | Rastreabilidade principal |
|---|---|---|
| R01 | Confirmado: produto próprio, janela nativa flutuante e residente. | T009, T037–T041, T065–T076 |
| R02 | Confirmado: providers comerciais e locais substituíveis. | T017–T019, T051 |
| R03 | Confirmado: Computer Use com LLM textual no escopo suportado. | T024–T029, T052 |
| R04 | Confirmado: xa11y é a fonte primária de percepção e ação semântica. | T004, T024–T028 |
| R05 | Confirmado: shell, arquivos, apps, processos, clipboard e janelas. | T030–T036 |
| R06 | Confirmado: conversa, streaming, sessões e interrupção. | T013–T023, T037–T040 |
| R07 | Confirmado: push-to-talk e transcrição antes do LLM. | T042–T046 |
| R08 | Confirmado: Linux/Wayland, Windows e macOS desde a arquitetura. | T005–T007, T047–T049 |
| R09 | Confirmado: fork seletivo do Pi, sem CLI/TUI como produto. | T003, T012–T016 |
| R10 | Confirmado: autorização, limites, execução confiável e verificação. | T020–T023, T028–T029, T050–T054 |
| R11 | Confirmado: aplicação instalável e utilizável diariamente. | T055–T060 |

“Confirmado” nesta tabela significa alinhado como requisito e rastreado; não significa implementado ou validado em execução.

### Jornadas J01–J08

| Jornada | Resultado da revisão | Condições preservadas |
|---|---|---|
| J01 | Confirmada como jornada de conversa, escolha de provider/modelo e recuperação de sessão. | Persistência e retomada precisam ser demonstradas; troca de modelo no meio de um run é proibida. |
| J02 | Confirmada como jornada de projeto Rust com arquivos, Git, execução/testes e abertura no VS Code. | Detectar ferramentas ausentes, conflitos e trabalho existente; não instalar nem sobrescrever implicitamente. |
| J03 | Confirmada como busca de arquivo baixado “ontem”. | Usar fuso local, intervalo explícito e candidatos; mtime não prova download. |
| J04 | Confirmada como listagem de janelas, abertura do Discord e navegação semântica. | Usar conta/servidor de teste; tratar login, falta de acesso e árvore incompleta. |
| J05 | Confirmada como organização de arquivos com prévia e proteção contra colisões. | Preservar conteúdo e oferecer reversão quando tecnicamente possível. |
| J06 | Confirmada como alteração inofensiva de configuração com verificação e restauração. | Usar app de teste e resultado observável; não testar alteração irreversível. |
| J07 | Confirmada como push-to-talk em português com revisão de transcrição. | O áudio é transcrito antes do LLM e segue as mesmas aprovações do texto. |
| J08 | Confirmada como interrupção durante streaming, shell, acessibilidade ou transcrição. | Informar efeitos já concluídos, impedir efeitos seguintes e nunca prometer rollback inexistente. |

## Decisões abertas e pressupostos reversíveis

As seguintes pendências foram mantidas explícitas, sem bloquear o próximo spike:

- **A05:** provider comercial, modelo local, nomes, endpoints e fallback.
- **A06:** engine de STT, local/remoto, licença, distribuição de pesos e custo.
- **A07:** versões mínimas de SO, arquiteturas, permissões e formatos de instalador.
- **A09:** nome final, licença do produto e canal de releases.
- **Fixture e contas de teste:** versões de VS Code/Discord, fixture acessível e contas controladas serão fixadas em T004/T008/T052.
- **Orçamento:** não há valor numérico; qualquer gasto real exige decisão posterior e controle de custo.

Pressupostos adotados apenas para avançar de forma reversível:

- usar “Vox” como nome de trabalho nos documentos;
- usar a máquina Linux observada como referência inicial, sem reduzir o escopo de Windows/macOS;
- priorizar provas locais e textuais antes de chamadas comerciais;
- manter visão/OCR, TTS, wake word, automações programadas, execução sem usuário presente, privilégios administrativos automáticos e sincronização fora da v1;
- não instalar software, acessar contas reais ou executar efeitos irreversíveis durante a fase de alinhamento.

## Aceite da T001

- [x] R01–R11 foram revisados e continuam rastreados nas tarefas do plano.
- [x] J01–J08 foram revisadas com suas condições de segurança e verificação preservadas.
- [x] Nome provisório, referências funcionais/técnicas, apps de referência, hardware e limites de orçamento foram registrados.
- [x] Provider/modelo, STT, multiplataforma, licença, releases, fixture e orçamento numérico ficaram registrados como decisões abertas.
- [x] Nenhuma implementação, compatibilidade ou sucesso de provider foi declarado sem prova.
- [x] O próximo passo está desimpedido: T002, T003 e T004 podem iniciar; T005–T008 dependem das provas indicadas no backlog.
