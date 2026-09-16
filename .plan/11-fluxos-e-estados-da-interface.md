# Fluxos de interação e estados

Complementa a [especificação visual](10-interface-janela-flutuante.md) e os [contratos](03-contratos.md). Os fluxos são requisitos de implementação; os textos são microcopy proposta para português brasileiro.

## 1. Separar janela, conversa e execução

Estado da janela: hidden / invocation / conversation / compact / preferences / history. Estado de run: idle / thinking / executing / awaiting_approval / waiting_user / cancelling / completed / failed / cancelled. Estado de voz: idle / recording / transcribing / review / failed. Cada dimensão é independente, com invariantes explícitas; “janela escondida” não equivale a “run cancelado”.

Invariantes: apenas um run com efeitos; apenas uma captura; Parar alcançável durante efeitos; aprovação pendente bloqueia ferramentas; alterações visuais não despacham ações; replay do histórico não reexecuta ferramenta; receber evento não ativa janela sem gesto do usuário. Eventos repetidos são deduplicados por run_id/seq. Eventos de run anterior não alteram a faixa do run atual.

## 2. Primeiro uso

1. Abrir janela com breve descrição: “Converse e realize tarefas no seu computador”. Ação “Configurar assistente”.
2. Selecionar serviço ou endpoint local. Formulário mostra nome do campo, modo de armazenamento da chave e “Testar conexão”. Não listar dezenas de providers numa grade inicial: busca/lista simples.
3. Validar conexão e selecionar um modelo compatível; explicar quando só pode conversar ou não oferece chamadas estruturadas suficientes.
4. Mostrar permissões por capacidade: acessibilidade para operar apps, microfone para voz. Pedir cada permissão quando necessária; negar microfone não bloqueia texto/shell autorizados.
5. Oferecer ação inofensiva demonstrável: “Mostrar as janelas abertas”. Exibir o que foi observado, sem enviar conteúdo adicional por antecipação.
6. Chegar à janela de invocação; setup pode ser retomado em Preferências. Não criar conta Vox obrigatória.

Falhas: endpoint inacessível mantém campos e permite tentar novamente; chave inválida não ecoa segredo; permissão negada mostra “Abrir configurações do sistema” quando disponível e “Continuar sem esta capacidade”.

## 3. Pedido por texto

Invocar → focar campo por gesto explícito → digitar → Enviar → registrar mensagem/run → expandir uma vez → pensar → executar/verificar → responder. Texto é limpo do campo apenas após aceitação local do turno; falha antes disso preserva rascunho. Duplo clique/Enter repetido não cria dois runs.

Enquanto pensa: “Preparando a tarefa…”; enquanto executa, frase baseada em evento (“Abrindo o VS Code…”). Se tarefa exige esclarecimento: “Em qual pasta devo criar o projeto?” com campo habilitado; resposta vinculada ao mesmo run. Não pedir confirmação para ações já autorizadas e reversíveis apenas porque uma ferramenta será usada.

Sucesso: “Projeto criado em ~/Projetos/exemplo. Os testes passaram.” somente se arquivos e exit code/pós-condição confirmarem isso. Parcial: “Criei os arquivos, mas os testes falharam.” com Ver erro e Abrir pasta. Nunca transformar término do stream em sucesso automático.

## 4. Pedido por voz

Pressionar push-to-talk → capturar → mostrar “Ouvindo…” e duração real → soltar → “Transcrevendo…” → review ou envio textual conforme preferência → fluxo normal. Em modo de revisão, mostrar transcrição editável e “Enviar”; em modo direto, registrar o texto resultante no histórico antes de efeitos. Deixar claro no onboarding qual modo está ativo; proposta padrão: revisão ativada até o usuário desativar.

Esc enquanto grava descarta captura; Esc enquanto transcreve cancela STT. Microfone também oferece alternativa “Iniciar gravação”/“Finalizar gravação” acionável por teclado, para não depender apenas de manter tecla pressionada. Limite proposto de captura: 60 segundos, com aviso aos 50; confirmar no spike. Não reproduzir áudio por padrão.

Falhas: “Não consegui ouvir sua fala. Tente novamente ou digite.” para áudio vazio; “Microfone indisponível. Escolha outro dispositivo.” para erro de hardware. Transcrição incorreta não pode ser reparada silenciosamente inventando alvo; pedir esclarecimento quando relevante. Voz não bypassa aprovações.

## 5. Aprovação contextual

1. Broker detecta ação que requer aprovação e publica payload estruturado.
2. UI mostra verbo, alvo exato, motivo, consequência e preview/diff quando disponível. Texto do modelo não define autorização.
3. Botões específicos: “Substituir arquivo”, “Enviar mensagem”, “Cancelar alteração”. Não usar apenas “OK”.
4. Foco inicial fica na descrição/ação segura; Enter que enviou mensagem não atravessa para confirmar aprovação recém-aberta.
5. Aceitar envia approval_id e contexto correspondente; broker revalida. Mudança de alvo/argumentos invalida aprovação e requer nova apresentação.
6. Negar encerra a ação pendente e informa o core; não substitui por outra ferramenta para conseguir o mesmo efeito.

Aprovação não expira visualmente com auto-dismiss. Se run for cancelado, mostrar “Solicitação cancelada” e desabilitar decisão. Em compacto, nenhum efeito novo até usuário expandir e decidir. Conteúdo de preview é redigido, paginado e não executável.

## 6. Parada e visibilidade

Clique Parar → UI muda imediatamente para “Interrompendo…” → bloquear dispatch no broker → cancelar provider/workers → reconciliar efeitos → mostrar “Interrompido” e resumo factual. O botão não desaparece enquanto o cancelamento está pendente; pode ficar desabilitado para repetição com estado legível.

Se chamada do SO não permitir abort: “A ação atual ainda está terminando. Nenhuma nova ação será iniciada.” Quando houver certeza, atualizar resultado. Botão não afirma desfazer. Interromper não remove a conversa nem perde rascunho.

Ocultar sem run: hidden. Ocultar durante run: compacto, salvo controle alternativo validado para ocultação total. Sair durante run: pedir encerramento ao supervisor e comunicar efeito incerto; se um worker precisar ser encerrado à força, somente processos próprios são alvos e o journal preserva incerteza.

## 7. Teclado e foco

| Entrada | Contexto | Resultado |
|---|---|---|
| Atalho global configurável | Qualquer app, se suportado | Invoca/expande; não inicia ação |
| Atalho push-to-talk configurável | Habilitado e disponível | Captura enquanto pressionado |
| Enter | Compositor, sem IME, run pronto | Envia texto |
| Shift+Enter | Compositor | Nova linha |
| Esc | Menu aberto | Fecha menu e devolve foco ao acionador |
| Esc | Captura/STT | Cancela voz sem executar |
| Esc | Run ativo, sem overlay/voz | Solicita Parar |
| Esc | Aprovação | Nega ação pendente, não a aprova |
| Esc | Idle | Oculta janela e preserva rascunho |
| Tab/Shift+Tab | Janela | Percorre controles visíveis em ordem lógica |

Prioridade de Esc: voz → menu/overlay → aprovação → run ativo → ocultar idle. Atalhos globais não recebem combinação fixa obrigatória antes de testar conflitos/portais; onboarding permite editar e indica disponibilidade. Preferências e Histórico têm Voltar, preservando posição/rascunho. Ao retornar por conclusão automática, não focar campo à força.

## 8. Preferências e histórico

Preferências substitui temporariamente o conteúdo principal, com Voltar e quatro grupos: Modelo e conexão; Voz e atalhos; Aparência e janela; Privacidade e dados. Não abrir coleção de janelas auxiliares. Alterações seguras (tema/texto) aplicam de imediato; provider/atalho/capacidade em uso aguardam run terminar ou exigem que usuário pare a tarefa.

Histórico: lista por recência com título derivado de intenção, data local e estado; busca simples; Abrir, Nova conversa e menu de exportar/apagar. Título automático não é evidência de sucesso. Apagar conversa ativa exige encerrá-la primeiro. Exportar mostra destino e redaction; exclusão segue política de dados, sem enviar histórico para serviço externo.

## 9. Catálogo de estados e mensagens

| Estado | Texto exemplo | Ação principal | Persistência |
|---|---|---|---|
| Vazio/configurado | Como posso ajudar? | Escrever/falar | Até pedido |
| Pensando | Preparando a tarefa… | Parar | Enquanto houver run |
| Executando | Organizando arquivos · Downloads | Parar | Estado vindo do broker |
| Esperando usuário | Qual destas pastas você quer usar? | Responder | Até resposta/cancelamento |
| Aprovação | Substituir este arquivo? | Decisão específica | Até decisão válida |
| Rede indisponível | A conexão caiu. Sua conversa foi preservada. | Tentar novamente | Sem retry de efeitos |
| Modelo incompatível | Este modelo não consegue usar as ferramentas necessárias. | Escolher modelo | Até seleção |
| Capability ausente | Não consigo controlar esta janela neste ambiente. | Ver alternativa | Sem falso sucesso |
| Permissão revogada | O acesso aos aplicativos foi desativado. | Rever permissão | Ferramentas afetadas bloqueadas |
| Concluído | Tarefa concluída. | Ver resultado | No histórico |
| Parcial | Os arquivos foram criados; a execução falhou. | Ver erro | No histórico |
| Cancelado | Interrompido. Duas ações já foram concluídas. | Ver atividade | Contagem somente real |
| Reinício após crash | A tarefa anterior foi interrompida. | Revisar estado | Não retomar automaticamente |

“Tentar novamente” inicia nova tentativa após revalidar o estado; não faz replay cego de efeitos. Erro técnico completo fica em detalhes redigidos, associado a identificador local para suporte.

## 10. Casos de aceite da interface

UI01 abrir/ocultar/reabrir conserva rascunho; UI02 streaming não rouba scroll; UI03 Enter IME não envia; UI04 compactar conserva run; UI05 Parar impede novos efeitos; UI06 aprovação não aceita Enter residual; UI07 monitor desconectado mantém janela acessível; UI08 texto 200% não corta decisão; UI09 voz cancelada não cria turno; UI10 troca de modelo durante run é bloqueada; UI11 evento antigo não substitui run atual; UI12 erro parcial mostra efeitos já ocorridos; UI13 leitor de tela identifica controles e não anuncia cada token; UI14 Ocultar durante run conserva acesso a Parar; UI15 crash/restart não reexecuta; UI16 renderer idle fica estável.

Mapeamento: UI01–UI04/T068–T070; UI05–UI06/T072; UI07–UI08/T066+T074; UI09/T073; UI10/T071; UI11/T077; UI12/T072; UI13/T074; UI14/T069; UI15/T081; UI16/T076. Todos devem gerar evidência de teste, não apenas screenshot.
