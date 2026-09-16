# Matriz de plataforma e evidência

| Ambiente | Código de compilação | Observação real | Estado |
|---|---|---|---|
| Linux Ubuntu 24.04, GNOME/Wayland, x86_64 | `cargo build -p vox-desktop` | janela e adapter xa11y observados; janelas de GNOME, Chromium, Chrome, OpenCode, Codex e terminal listadas | validado localmente |
| Linux X11 | CI/build configurado | sessão X11 não estava disponível nesta máquina | pendente de execução real |
| Linux KDE/Wayland | CI/build configurado | sessão KDE não estava disponível nesta máquina | pendente de execução real |
| Windows 11, UIA | workflow de build/teste | nenhuma máquina Windows disponível nesta sessão | não afirmar suporte |
| macOS, AXUIElement | workflow de build/teste | nenhum macOS disponível nesta sessão | não afirmar suporte |

`xa11y` mantém diferenças honestas: ausência de AT-SPI/UIA/AX retorna diagnóstico; a fixture `MockDesktop` é usada somente nos testes e nunca é rotulada como observação real.

## Critério de promoção

Um ambiente só passa para suportado depois de registrar build, janela nativa, foco, teclado, clipboard, permissão, cancelamento, fixture e uma jornada completa no pacote instalável. O workflow multiplataforma comprova compilabilidade, não comprova a experiência do desktop.
