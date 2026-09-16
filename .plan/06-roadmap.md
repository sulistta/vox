# Roadmap e recursos

## Fases e gates

| Fase | Tarefas | Saída / gate |
|---|---|---|
| F0 — Viabilidade | T001–T008 | G0: referências fixadas, runtime escolhido, provas nos SOs, riscos e matriz inicial |
| F1 — Fundação | T009–T016 | G1: UI/core/broker conectados, política básica, fork seletivo e cancelamento de infraestrutura |
| F2 — Fatia vertical | T017–T020, T024–T028, T030, T037–T038 | G2: pedido textual → ação semântica → verificação, parada funcional, sem CLI Pi |
| F3 — Alpha funcional | T021–T023, T029, T031–T036, T039–T041 | G3: sessões, ferramentas e jornadas J01–J06 com modelo local e comercial |
| F4 — Voz e plataformas | T042–T049 | G4: push-to-talk e capacidades reais nos três SOs; limitações registradas |
| F5 — Qualidade e beta | T050–T057 | G5: segurança, resiliência, avaliação, instaladores e beta diário aprovados |
| F6 — v1 | T058–T060 | G6: release validada, documentação, manutenção e todos os requisitos rastreados |
| Futuro | T061–T064 | Planejamento separado, sem bloquear v1 |

As fases agrupam entregas; as dependências no backlog são a ordem vinculante. Testes e segurança acompanham implementação desde F1; F5 consolida a validação, não inicia a preocupação com qualidade.

## Caminho crítico

T002/T003/T004/T005 → T008 → T009/T010 → T011/T012/T013/T014 → T016/T017 → T020/T024/T025/T027/T028 → T037/T038 → T052 → T055/T056/T057 → T058/T059/T060.

Outros caminhos também podem bloquear release: portabilidade T006/T007/T048/T049; voz T042–T046; segurança T050; recuperação T054. Um problema de acessibilidade em app alvo deve ser descoberto em F0, não após desenhar toda a UI.

## Dimensionamento

Planejamento inicial para uma pessoa experiente: aproximadamente 7–12 meses úteis de execução focada para v1, com incerteza alta e reserva de 25–35% para portabilidade/distribuição. Não é soma contratual das estimativas do backlog. Reestimar após G0 e G2 usando throughput real. Equipe de 2–3 pessoas pode dividir core, desktop e validação, mas os gates de integração permanecem sequenciais.

Recursos necessários: máquina Linux com sessões GNOME/KDE Wayland e X11 de referência; Windows e macOS reais para permissões, foco e áudio; CI para builds nos três SOs; contas de teste em apps; acesso a pelo menos um provider comercial e um modelo local de tool use; orçamento limitado de API; certificados/contas de distribuição se exigidos pelo canal; dispositivo de áudio e cenários de múltiplos monitores. Fixar hardware e orçamento em T001/T008; valores dependem de escolhas ainda abertas.

## Regra de bloqueio

Spike de viabilidade deve ter timebox de até cinco dias úteis e produzir relatório reproduzível, mesmo negativo. Se xa11y não atender requisito central, documentar evidência e avaliar adapter nativo complementar sem modificar a dependência; mudanças de stack ou retirada de suporte Linux/Wayland precisam de decisão de produto explícita. Não ocultar requisito falho sob “suporte parcial” sem mapear o impacto nas jornadas.

## Integração do detalhamento aos gates

| Fase / gate | Pacotes adicionais obrigatórios | Evidência adicional |
|---|---|---|
| F1 / G1 | T065 | Protótipo nativo dos modos e geometria; direção visual especificada |
| F2 / G2 | T066–T068 | Janela real, tokens, compositor, scroll e streaming; T038 depende dessa integração |
| F3 / G3 | T069–T072, T074, T076–T080 | Compacto, histórico, preferências, aprovação, acessibilidade, reducer e fixtures técnicas |
| F4 / G4 | T073 | Voz integrada visualmente e por teclado |
| F5 / G5 | T075, T081–T082 | Suíte da interface, falhas entre processos e pacote instalado |
| F6 / G6 | Todos acima | UX01–UX10 e UI01–UI16 rastreados junto a R01–R11 |

T065 pode começar após T001 como protótipo descartável; integração de produção segue T008/T009. Atualização do caminho crítico de UI: T065 → T066/T067 → T068 → T038 → T069/T072 → T074 → T076 → T041. O caminho da beta também passa por T073 → T075 → T082 → T057. Não postergar design/foco/cancelamento para um polimento após release.

As novas tarefas decompõem parte do esforço antes agregado em T037–T040/T021/T054–T056; não somar estimativas integralmente em duplicidade. Reestimar após protótipo e spikes, separando escopo novo de detalhamento de trabalho já previsto.
