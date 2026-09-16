# Plataformas e experiência

## Matriz a validar em máquinas reais

Todos os campos abaixo são alvos de teste, não compatibilidade já comprovada. Definir versões mínimas e arquiteturas em T008 com base nas dependências fixadas.

| Ambiente | Semântica prevista | Áreas de maior incerteza | Evidência requerida |
|---|---|---|---|
| Linux GNOME Wayland | xa11y / AT-SPI2 | Atalho global, ativação/foco, tray, clipboard, apps sandboxed | T005 + T047 + E2E |
| Linux KDE Wayland | xa11y / AT-SPI2 | Diferenças de compositor/portais, atalhos e janelas | Mesma suíte e limitações registradas |
| Linux X11 de referência | xa11y / AT-SPI2 | Variações de desktop e permissões | Regressão explícita |
| Windows | xa11y / UI Automation | Integridade de processos, UAC, atalhos e distribuição | T006 + T048 + E2E |
| macOS | xa11y / APIs de acessibilidade | TCC, assinatura, foco e permissões por versão | T007 + T049 + E2E |

Separar acesso semântico de captura de tela, injeção de teclado, foco e atalhos globais. Êxito do AT-SPI não prova que essas outras capacidades funcionam em Wayland. Detectar capability em runtime e consultar APIs/portais documentados do ambiente. Não depender de XWayland como resposta ao requisito de Wayland.

Sem atalho global disponível, oferecer abertura pelo launcher e controle push-to-talk na janela. Sem tray, manter forma explícita de reabrir e sair. Sem foco programático, orientar o usuário e continuar após observar o alvo. A limitação deve aparecer no onboarding e no relatório de suporte; avaliar se ainda permite cumprir o gate de produto. Nunca recorrer silenciosamente a root, scripts de bypass ou clique em coordenada.

## Interface principal

A interface principal é uma **janela flutuante nativa, minimalista e moderna**, especificada em [Interface: janela flutuante](10-interface-janela-flutuante.md). O documento define medidas propostas, tokens, tipografia, anatomia, comportamento do desktop e aceite visual. Os [fluxos e estados](11-fluxos-e-estados-da-interface.md) definem interação, teclado, microcopy e recuperação; são a referência detalhada para T037–T040 e T065–T076.

Modos: invocação 440 × 176 pt; conversa 440 × 520 pt; acompanhamento compacto 360 × 88 pt. São propostas a validar em egui e ajustar por escala/área útil. A conversa ocupa o centro; preferências e histórico são secundários. Abertura traz à frente quando permitido, e Manter acima é opt-in. A UI não rouba foco durante ações em outros apps.

Ocultar durante run preserva acesso a Parar, normalmente pelo modo compacto. Sair encerra captura e processos próprios. Aprovação suspende efeitos e nunca é aceita implicitamente por foco ou Enter residual. Sem atalho global, usar launcher e controle na janela; sem posicionamento programático, respeitar compositor. Não prometer paridade de APIs de janela entre SOs.

## Voz

Fluxo: key-down inicia captura → indicador e nível de áudio → key-up encerra → STT → transcrição → turno textual. Ignorar auto-repeat, tratar key-up perdido, mudança de dispositivo, suspensão, limite de duração e permissão revogada. Áudio não fica gravando após cancelamento/fechamento.

Definir interface Transcriber independente dos providers de LLM. Comparar engine local e serviço remoto em T042; selecionar pelo menos uma implementação para v1 e explicitar se há dependência de rede. Fluxo totalmente offline requer também STT local, além de LLM local. Modelo de STT é dependência separada, com licença, tamanho, download verificado e remoção.

Português como idioma inicial; fixtures com nomes de apps e diretórios. Transcrição vazia não executa. Permitir edição antes de envio como configuração e pedir esclarecimento em ambiguidade de alvo. Ações de risco continuam sujeitas à mesma política de texto. Resposta textual é obrigatória; leitura por TTS é posterior.

Revisão de voz: padrão proposto é revisar a transcrição antes de enviar, com preferência explícita para envio direto. Fluxo por teclado também permite iniciar/finalizar captura sem manter tecla pressionada. Consulte a seção 4 dos [fluxos](11-fluxos-e-estados-da-interface.md).
