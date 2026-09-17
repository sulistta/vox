<!-- Hallmark · revisão documental: Philosophy 5 / Hierarchy 5 / Execution 4 / Specificity 5 / Restraint 5 / Variety 4. Autoavaliação da especificação; sem renderização ou teste visual realizado. -->
# Especificação da interface — janela flutuante

## 1. Direção de produto

A interface principal do Vox **é uma janela flutuante nativa, minimalista e moderna**, disponível por atalho ou launcher. Ela deve parecer um pequeno assistente pessoal sobre o desktop: uma superfície bem acabada para falar, escrever, acompanhar uma ação e interrompê-la. A conversa é o centro da experiência.

Minimalismo significa hierarquia clara, poucos controles permanentes e detalhes acessíveis quando necessários. Modernidade vem de proporção, tipografia, espaçamento, resposta imediata e estados consistentes. Não depende de transparência, efeitos gráficos ou animação contínua. Evitar dashboards, sidebar permanente, terminal como tela inicial, cartões para cada frase, mascote, orb animado, gradiente decorativo e linguagem de painel administrativo.

Especificação proposta a implementar e validar, não imagem de um produto pronto. As medidas e cores abaixo são parâmetros de projeto ajustáveis após protótipo. Requisito confirmado: janela flutuante minimalista/moderna. Paleta, fonte, posição e geometria são propostas, não preferências já confirmadas do usuário.

Hallmark aplicado à hierarquia, estados e contenção visual. Regras de landing page, hero, footer, DOM/CSS e breakpoints de celular não se aplicam à janela Rust/egui. Não criar site, WebView ou falso chrome de navegador para materializar esta especificação.

## 2. Modos e geometria

Unidade: ponto lógico do toolkit, independente de pixels físicos. Conteúdo deve respeitar escala de texto e DPI do SO.

| Modo | Tamanho inicial proposto | Conteúdo e uso |
|---|---|---|
| Invocação vazia | 440 × 280 pt | Cabeçalho discreto, “Como posso ajudar?”, compositor e identificação do modelo |
| Conversa | 440 × 520 pt | Cabeçalho, histórico rolável, faixa de execução e compositor fixo |
| Acompanhamento compacto | 360 × 88 pt | Ação atual, app alvo, Expandir e Parar; aparece por escolha do usuário |
| Aprovação | Largura atual, altura até limite útil | Conversa e resumo concreto com botões; área de detalhes rolável |
| Preferências/histórico | 560 × 560 pt quando houver espaço | Uma tela secundária por vez dentro da mesma janela, com Voltar |

Largura normal ajustável entre 360 e 640 pt; altura útil máxima de 720 pt e sempre limitada à área de trabalho menos margem de 16 pt em cada borda. Em área útil menor, permitir largura até 320 pt; reduzir margens e rolar conteúdo, nunca cortar Parar ou botões de decisão. Escala de texto de 200% pode exigir janela maior/rolagem, sem texto sobre controles. O protótipo egui foi ajustado de 176 para 280 pt após uma captura real mostrar o compositor cortado; a altura mínima normal é 260 pt e só o modo compacto reduz esse limite.

Primeira abertura: centralizada horizontalmente no monitor ativo, em torno de 22% da altura útil a partir do topo, sem encostar em painel/dock. Depois preservar posição e tamanho escolhidos pelo usuário por monitor quando identificável. Se monitor sumir, reposicionar dentro da área útil do monitor atual. Não pressupor que Wayland permita posicionamento absoluto; quando não permitir, aceitar posicionamento do compositor e registrar capability.

A janela cresce de invocação para conversa após o primeiro envio, preservando a posição superior quando o ambiente permitir. Crescimento é uma transição única de layout, sem tween contínuo das dimensões nativas. Não redimensionar a cada token. Após ajuste manual, respeitar tamanho até nova escolha do usuário. Recolher só por comando explícito; não esconder respostas longas automaticamente.

## 3. Anatomia da janela

Ordem vertical: cabeçalho → conversa → execução/aprovação, se existente → compositor → identificação do modelo. Cabeçalho, compositor e Parar permanecem alcançáveis; o histórico absorve a rolagem.

### Cabeçalho

Altura-base 40 pt. À esquerda, nome “Vox” em 13 pt semibold, sem logo grande. À direita, menu Mais e **Recolher**. Toda área livre do cabeçalho permite arrastar, preservando hitboxes dos botões. Arraste não é a única forma de mover: usar mecanismos nativos de janela e caminho por teclado disponíveis no SO.

Menu Mais: Nova conversa, Histórico, Preferências, Manter acima e Sair. Marcar estado de Manter acima e explicar quando indisponível. **Recolher** reduz a própria janela para o acompanhamento compacto e mantém os controles Expandir e, durante um run, Parar visíveis. Enquanto não houver uma rota de reativação validada (tray, atalho global ou integração equivalente), a interface não oferece Ocultar/minimizar a única janela do Vox. “Sair” solicita interrupção de run ativo, mostra resultado incerto se existir, encerra captura e processos próprios.

Decoração própria discreta é desejada, condicionada à viabilidade de arrastar/redimensionar, sombras, acessibilidade e menus do SO. Usar decoração nativa caso customização prejudique esses recursos. Não desenhar botões falsos imitando macOS no Windows/Linux.

### Conversa

Margens horizontais 20 pt; espaços verticais entre turnos de 16–20 pt. Mensagens do usuário em superfície neutra suave, alinhadas à direita, ocupando no máximo 90% da largura interna. Respostas do assistente alinhadas à esquerda, diretamente sobre a superfície principal. Sem avatar repetido em cada turno; rótulos “Você”/“Vox” acessíveis distinguem autoria.

Corpo 14 pt e altura de linha aproximada de 21 pt. Suportar parágrafos, listas, links seguros, código monoespaçado e caminhos clicáveis. Markdown sem HTML ativo, imagens remotas automáticas ou conteúdo executável. Abrir link/arquivo exige gesto do usuário e resolução de destino; nunca executar comando ao clicar num bloco de código. Blocos longos têm rolagem horizontal própria, copiar e expandir; não alargam janela.

Autoscroll somente enquanto usuário estiver no final do histórico. Se subir para ler, preservar posição e mostrar “Novas mensagens” como controle para voltar ao final. Atualizações de streaming são agrupadas para renderização, não substituem texto já confirmado nem mudam o foco. Ações Copiar/Repetir ficam em menu acessível, sem depender de hover.

### Execução

Uma faixa discreta acima do compositor mostra ícone funcional, verbo concreto e alvo: “Executando testes · meu-projeto”. Botão **Parar** textual e sempre disponível. No máximo uma linha secundária de contexto; detalhes de ferramenta ficam em expansão “Ver atividade”. Não mostrar JSON, nomes internos de tool, raciocínio privado ou logs completos por padrão.

Se houver passos conhecidos, apresentar lista curta e factual com pendente/em andamento/concluído/falhou. Não inventar porcentagem ou quantidade total quando o agente ainda planeja. Mostrar “3 ações concluídas” é permitido apenas se o contador vier de eventos reais. Em tarefas com saída longa, mostrar trecho recente e acesso ao log redigido.

### Compositor

Campo com rótulo visível “Mensagem” (12 pt) e placeholder “Peça algo ao seu computador”. Altura de texto começa em uma linha e cresce até cinco linhas; além disso, rolagem interna. Área-base do compositor aproximadamente 76–100 pt, incluindo ações e espaços.

Enter envia; Shift+Enter insere quebra; Enter durante composição IME não envia. Texto vazio/espaços não envia. Botão Enviar com nome acessível; microfone separado com tooltip “Segure para falar”. Ações com alvo mínimo de 44 × 44 pt, mesmo que ícone visual tenha 18 pt. Desktop sem toque também preserva essa área para precisão.

Durante run ativo, edição de rascunho continua possível, mas envio fica indisponível com indicação “Pare a tarefa para enviar outro pedido”. Rascunho é preservado. Steering e fila de novas mensagens ficam fora da v1; essa regra simplifica a relação entre intenção, efeito e autorização. Resposta a pergunta do agente é habilitada no estado aguardando usuário e continua o run correto.

### Identificação do modelo

Linha discreta abaixo do campo: nome curto do modelo e indicador textual “Local” ou nome do serviço. Clicar abre seletor simples. Nome completo aparece em tooltip/detalhes; truncar visualmente, mantendo descrição acessível. Troca durante execução é bloqueada com explicação; não altera provider silenciosamente. Falha de conexão aparece junto ao seletor e no contexto da tentativa.

## 4. Tokens visuais propostos

Tema segue o SO por padrão; Claro/Escuro/Sistema em Preferências. Uma família sans para a interface e uma mono para código, ambas embutidas/licenciadas para evitar dependência de download. Candidatas: Noto Sans e Noto Sans Mono; validar cobertura e licença ao incorporar, com fallback para símbolos/idiomas. Não usar fonte display de marketing na janela.

| Token semântico | Claro | Escuro | Aplicação |
|---|---|---|---|
| surface.base | #FAFAF9 | #181A1D | Fundo principal opaco |
| surface.raised | #FFFFFF | #22252A | Campo, menu e aprovação |
| surface.subtle | #F0F1F2 | #2B2F35 | Mensagem do usuário e hover |
| text.primary | #20242B | #F3F4F6 | Texto principal |
| text.secondary | #525B67 | #B9C0CA | Labels e contexto |
| border.subtle | #D4D8DE | #454C56 | Separação decorativa, não único sinal de foco |
| accent.fill | #2459C4 | #9ABBFF | Ação principal e seleção |
| accent.text | #FFFFFF | #142340 | Texto sobre accent.fill |
| focus.ring | #174DA8 | #B4CDFF | Foco externo com espaçamento |
| status.success | #21683D | #87D7A4 | Sucesso com texto/ícone |
| status.warning | #86540A | #F0C575 | Atenção com texto/ícone |
| status.error | #B42335 | #FFA1AA | Erro e ação de interromper |

Valores são candidatos a verificar por contraste no protótipo. Meta: texto normal ≥4,5:1; ícones funcionais/foco ≥3:1 nos pares efetivamente usados. Se focus.ring conflitar com botão preenchido, usar anel de duas camadas com separador surface.base. Borders decorativas não garantem contraste para delimitar controles; usar combinação apropriada de fundo, texto, label e borda funcional verificada.

Escala espacial: 4/8/12/16/20/24/32 pt. Raio da janela 16 pt quando compositor permitir; compositor/cartões 10 pt; controles 8 pt. Bordas de 1 pt constantes em todos os estados, sem deslocamento ao focar. Sombra única discreta fornecida pelo SO quando possível; fallback em borda opaca se sombra não suportada. Não exigir blur, transparência ou recorte arredondado para funcionar.

Tipografia: título de tela 18/24 semibold; texto 14/21 regular; controle 13/18 medium; legenda 12/16; código 12/18 mono. Sem textos essenciais abaixo de 12 pt. Estado e importância vêm de texto/peso/espaço além de cor. “Parar” não disputa com Enviar durante execução: envio indisponível, Parar é o controle ativo de maior evidência.

## 5. Estados de componentes

| Estado | Campo/botão | Comunicação |
|---|---|---|
| Padrão | Geometria estável e label legível | Intenção da ação |
| Hover | Mudança discreta de superfície | Sem mover layout |
| Foco por teclado | Anel externo imediato | Ordem previsível de navegação |
| Pressionado | Superfície mais marcada | Sem atraso no dispatch |
| Desabilitado | Aparência distinta, motivo acessível | Não depender apenas de baixa opacidade |
| Carregando | Indicador funcional e label preservado | Ação em andamento; evitar duplo envio |
| Erro | Ícone, mensagem e ação de recuperação | Explicar o que falhou |
| Sucesso | Confirmação curta | Não apagar informação necessária para auditoria |

Nem todo botão possui estado persistente de sucesso; mapear estado no componente responsável pelo resultado, sem inventar semântica. Foco e Parar nunca aguardam animação. Uma transição curta de opacidade para menus (120 ms), feedback de pressão e indicador funcional de captura/execução bastam. Sem pulsação decorativa ou animação por token. Respeitar movimento reduzido; renderer fica ocioso quando nada muda.

## 6. Comportamento no desktop

“Flutuante” não significa exigir always-on-top permanente. Ao invocar, trazer a janela à frente quando permitido. “Manter acima” é opt-in; não roubar foco periodicamente. Antes de ação em outro app, preservar identidade do alvo, permitir que ele receba foco e impedir que o próprio Vox seja escolhido como alvo implícito.

Clique fora não envia, cancela ou descarta texto; por padrão a janela permanece visível sem foco. O controle disponível é **Recolher**: ele sempre converte para acompanhamento compacto e mantém a única janela alcançável, tanto ociosa quanto durante execução. Ocultação/minimização por controle do Vox só entra depois de existir rota de reativação validada no ambiente e, durante run, outro caminho de Parar igualmente validado. Se captura estiver ativa, uma futura ocultação total interrompe captura e descarta áudio ainda não confirmado.

Modo compacto não aceita texto, não mostra histórico e não altera o run. Expandir recupera exatamente a conversa, rascunho e posição de leitura. Aprovação surgida nesse modo mostra “Preciso da sua confirmação” e Expandir; efeitos ficam suspensos. Não abrir janela por cima do usuário nem aprovar por atalho genérico.

Conclusão em modo compacto permanece como “Concluído · Ver resultado”, sem sumir automaticamente. Erro e ação incerta também permanecem. Notificação do SO é opcional, sem conteúdo sensível, se janela não estiver visível; não é requisito para saber o estado.

## 7. Esboços estruturais

Os blocos abaixo descrevem a distribuição funcional; não simulam chrome de navegador nem especificam pixels finais.

```text
INVOCAÇÃO (440 × 280)
Vox                                      Mais  Recolher
Como posso ajudar?
Mensagem
[ Peça algo ao seu computador                         ]
[Microfone]                                    [Enviar]
Modelo de referência · Local
```

```text
CONVERSA (440 × 520)
Vox                                      Mais  Recolher
                              Crie um projeto Rust...
Vou criar os arquivos e executar os testes.
[histórico rolável; espaço flexível]
Executando testes · meu-projeto                  [Parar]
Ver atividade
Mensagem
[                                                    ]
[Microfone]                                    [Enviar]
Modelo de referência · Local
```

```text
ACOMPANHAMENTO (360 × 88)
Executando testes                      [Expandir] [Parar]
A janela continua visível; a tarefa segue sob seu controle.
```

```text
APROVAÇÃO (na conversa)
Substituir arquivo?
~/Projetos/exemplo/Cargo.toml
O arquivo já existe. A alteração substituirá o conteúdo.
[Ver diferenças]
[Cancelar alteração]                         [Substituir]
```

Textos demonstrativos acima não representam ações já executadas. Medidas devem ser verificadas com fonte, escala e decoração final; se não couber, ajustar geometria sem remover controles essenciais.

## 8. Aceite visual e funcional

T065–T076 implementam e validam esta especificação. Entregar capturas reais do protótipo egui em claro/escuro, modos invocação/conversa/compacto/aprovação/erro/voz, larguras 320/360/440/640 pt e escala de texto 100/150/200%. Inspecionar acentos, nomes longos, foco, truncamento, compositor multilinha, código e estado de conexão.

Aceite: janela principal reconhecível como assistente flutuante; nenhuma sidebar permanente; máximo de três ações permanentes no cabeçalho; modelo legível sem dominar; Parar acessível durante run; nenhum controle cortado; nenhuma mudança de layout por token; nenhum foco roubado; resultado e falha distinguíveis; renderer em repouso não faz animação contínua. Protótipo bonito sem esses comportamentos não fecha a tarefa.
