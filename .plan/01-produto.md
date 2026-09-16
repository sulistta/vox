# Produto, requisitos e escopo

## Objetivo

Assistente desktop independente, residente, conversacional e multimodelo, que combina acessibilidade, arquivos, shell e recursos nativos para executar intenções do usuário. Rust com egui/eframe forma o aplicativo; TypeScript preserva componentes úteis de um fork do Pi. xa11y é dependência inicialmente sem modificações. Não distribuir o CLI do Pi como motor do produto.

“Universal” descreve a arquitetura e a diversidade de tarefas; não promete controle de toda interface existente. Aplicativos sem semântica suficiente precisam retornar limitação explícita e oferecer assistência manual. OCR/visão futuros devem produzir observações textuais para preservar independência de multimodalidade.

## Requisitos rastreáveis

| ID | Requisito | Entrega principal |
|---|---|---|
| R01 | Produto próprio; janela nativa flutuante, minimalista e moderna; residência | T009, T037–T041, T065–T076 |
| R02 | Providers substituíveis, comercial e local | T017–T019, T051 |
| R03 | Computer Use completo no escopo suportado com LLM textual | T024–T029, T052 |
| R04 | xa11y como fonte primária de percepção e ação semântica | T004, T024–T028 |
| R05 | Shell, arquivos, apps, processos, clipboard e janelas | T030–T036 |
| R06 | Conversa, streaming, sessões e interrupção | T013–T023, T037–T040 |
| R07 | Push-to-talk e transcrição antes do LLM | T042–T046 |
| R08 | Linux/Wayland, Windows e macOS desde a arquitetura | T005–T007, T047–T049 |
| R09 | Fork seletivo do Pi; sem CLI, TUI ou wrapper como produto | T003, T012–T016 |
| R10 | Execução confiável, autorização e verificação de resultados | T020–T023, T028–T029, T050–T054 |
| R11 | Instalável e utilizável diariamente | T055–T060 |

## Jornadas de aceite

- J01: abrir o assistente, escolher provider/modelo, conversar e retomar a sessão após reiniciar.
- J02: pedir um projeto Rust; selecionar diretório, criar arquivos, inicializar Git, executar projeto/testes e abrir pasta no VS Code. Detectar ferramentas ausentes e conflitos; não instalar software nem sobrescrever trabalho implicitamente.
- J03: localizar arquivo baixado “ontem”; usar fuso local e intervalo de datas explícito, metadados disponíveis e mostrar candidatos. Data de modificação não prova data de download.
- J04: listar apps/janelas, abrir Discord e navegar até servidor de teste usando semântica; confirmar resultado. Tratar login, falta de acesso e árvore incompleta.
- J05: organizar arquivos após apresentar movimentações; preservar conteúdo e lidar com colisões. Oferecer reversão quando tecnicamente possível.
- J06: alterar configuração inofensiva de app de teste, verificar novo valor e restaurar estado no teste.
- J07: pressionar atalho, falar em português, soltar, revisar transcrição quando necessário e receber resposta. Sem envio de áudio ao LLM textual.
- J08: interromper uma sequência durante streaming, shell, acessibilidade ou transcrição; informar ações já concluídas e impedir ações seguintes.

## Recortes

**Fatia vertical (G2):** uma plataforma Linux de referência, chat nativo, um provider textual, uma ação semântica real, executor mínimo controlado e botão de parada. Não é produto concluído.

**Alpha (G3):** ferramentas completas, modelo local e comercial, persistência, autorizações e jornadas J01–J06 em ambiente controlado. Windows/macOS já têm provas técnicas e builds básicos.

**Beta (G5):** voz, instaladores, matriz nos três sistemas, recuperação e testes de uso diário.

**v1 (G6):** todos R01–R11, jornadas aprovadas no escopo publicado, documentação, distribuição e suporte operacional.

**Após v1:** OCR/visão opcionais, TTS, wake word, automações programadas, marketplace/MCP, sincronização, memória longitudinal, operação remota e agentes simultâneos. Esses itens não bloqueiam a v1. Não incluir inicialmente execução sem usuário presente, privilégios administrativos automáticos ou bypass de restrições do sistema.

## Requisitos de experiência detalhados

UX01: janela compacta nativa, com conversa como área principal e sem sidebar permanente. UX02: três modos (invocação, conversa, acompanhamento) preservam o mesmo contexto. UX03: Parar permanece acessível em toda execução. UX04: interface não rouba foco do aplicativo operado. UX05: temas claro/escuro, contraste e texto escalável. UX06: histórico e preferências são telas secundárias. UX07: voz mostra captura/transcrição e oferece revisão. UX08: falhas, efeitos parciais e aprovações são claros e factuais. UX09: troca de modelo não ocorre no meio do run. UX10: teclado, leitor de tela, DPI e múltiplos monitores fazem parte do aceite.

Implementação e critérios em [interface](10-interface-janela-flutuante.md) e [fluxos](11-fluxos-e-estados-da-interface.md). J01/J07/J08 devem validar esses requisitos, além do resultado funcional das ferramentas.
