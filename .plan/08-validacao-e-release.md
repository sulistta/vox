# Validação e distribuição

## Pirâmide e ambientes

- Unitários: parsing de tool calls, política, redaction, estados, caminhos, orçamentos e normalização.
- Contratos: schemas TS/Rust, framing, versões, streaming, cancelamento, erros e capabilities.
- Integração: provider fake determinístico, broker real em diretório temporário, DB, processos filhos e worker de acessibilidade.
- GUI: app fixture acessível controlado com botões, campos, listas virtualizadas, diálogo e atualização de árvore; testes reais por SO.
- E2E: jornadas J01–J08 com app fixture e depois VS Code/Discord em contas de teste. Não executar ações irreversíveis reais para testar política.
- Avaliações LLM: mesmos objetivos, estado inicial e critérios com provider comercial e local; registrar modelo, versão, parâmetros, tokens, custo e repetições. Proibir payloads de imagem/áudio no adapter durante a suíte textual.

CI headless valida core e builds, mas não substitui sessão desktop real, microfone, portais ou permissões de instalação. Runners GUI dedicados/execução manual documentada são necessários. Dados de teste sintéticos; limpar arquivos, clipboard e processos ao final.

## Critérios mensuráveis propostos

Metas iniciais a confirmar em T008 e medir no hardware de referência; nenhuma está comprovada.

| Item | Gate |
|---|---|
| Compatibilidade | Todos R01–R11 vinculados a tarefas e evidências; matriz SO/app/versão publicada |
| Jornada textual controlada | ≥90% de sucesso por jornada/plataforma/modelo em 20 execuções; falhas classificadas, sem ação indevida |
| Apps reais J02/J04 | Pelo menos 10 execuções por combinação publicada; ≥90% sucesso; evidência de bloqueios legítimos |
| Autorização | 100% dos casos adversariais de teste bloqueados/aprovados conforme política; zero crítico aberto |
| Parada | UI reconhece Parar em p95 ≤100 ms; bloqueia novos dispatches em ≤250 ms; workers canceláveis encerram em ≤2 s |
| Ação não cancelável | Nenhuma ação subsequente; UI informa efeito pendente/incerto e reconciliado |
| Responsividade | Nenhuma operação de rede/SO bloqueia render; abrir janela residente em p95 ≤300 ms |
| Recursos | Alvo inicial idle ≤300 MiB RAM e ≤1% de um core, sem contar modelos locais/STT; medir separadamente |
| Áudio | Sem captura após soltar/cancelar; transcrição pt-BR avaliada por intenção correta em ≥90% de 50 frases |
| Recuperação | Reinício não repete automaticamente efeitos nem perde mensagens já confirmadas; migrations testadas |
| Uso diário | Cinco dias de dogfooding por SO, sessões de oito horas, sem crescimento contínuo de memória ou órfãos |

Latência LLM/STT depende de rede/hardware: medir p50/p95, tempo até primeiro token, fim da transcrição e duração de tarefa, separados do custo do app. Se meta se revelar inviável, registrar evidência e decisão antes de alterar o gate.

## Matriz de jornadas

J01/T051; J02/T052; J03/T031+T052; J04/T028+T052; J05/T032+T052; J06/T028+T052; J07/T046; J08/T023+T054. Repetir nas plataformas declaradas, incluindo GNOME e KDE Wayland. Para cada resultado: build, SO/compositor, app, modelo, pré-condições, passos, resultado esperado/real, evidência redigida e motivo de falha. Uma capacidade indisponível não conta como teste aprovado por skip sem justificativa e revisão de escopo.

## Release

Builds fixados por lockfiles e toolchains. Artefatos contêm UI, core próprio e runtime necessário; usuário não precisa instalar Node/Pi. Validar tamanho, permissões, DLLs/libs, licenças e hashes. Linux: escolher formatos após testar integração desktop e restrições de sandbox; Windows: instalador assinado conforme canal; macOS: bundle com permissões, assinatura/notarização conforme distribuição. Não prometer formatos antes de T008/T055.

Testar instalação limpa, atualização de dados, rollback compatível, desinstalação e escolha de apagar dados em cada SO. Separar update do app de update de modelos. v1 pode usar atualização manual por artefato assinado/verificado; updater automático só entra se assinatura, falhas e rollback estiverem testados.

Checklist de release: builds identificáveis; SBOM e notices; zero vulnerabilidade crítica conhecida sem resolução; documentação de permissões/limitações; changelog; recuperação e exportação; suporte/diagnóstico redigido; critérios quantitativos medidos; jornadas aprovadas; nenhum segredo em artefatos; validação final T060. App “funciona na máquina de desenvolvimento” não fecha release.

## Gate de interface flutuante

Executar UI01–UI16 dos [fluxos](11-fluxos-e-estados-da-interface.md) e conferir os tokens/modos da [especificação visual](10-interface-janela-flutuante.md). Cobrir 320/360/440/640 pt, escala textual 100/150/200%, temas claro/escuro, monitores/DPI diferentes, nomes longos, IME, histórico extenso, aprovação no compacto, microfone removido e conexão perdida.

Capturas devem ser do egui real; mockups e diagramas não provam integração. Aceite exige que Enviar/Parar/aprovação estejam visíveis, que foco/scroll sejam preservados e que nenhum modo dispare efeitos implicitamente. Medir contraste nos pares usados, consumo do renderer idle e latência do comando Parar. Testes de acessibilidade incluem teclado e leitor de tela por SO. Repetir smoke no pacote instalado (T082), não apenas no executável de desenvolvimento.

Rastreabilidade: UX01/T065+T066; UX02/T068+T069; UX03/T072; UX04/T066+T074; UX05/T067+T074; UX06/T070+T071; UX07/T073; UX08/T072; UX09/T071; UX10/T074. G6 não fecha com esses requisitos abertos.
