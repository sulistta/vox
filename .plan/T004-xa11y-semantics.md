# T004 — Relatório inicial de xa11y e semântica dos apps

- **Status:** em andamento; aceite ainda não fechado
- **Responsável:** Codex
- **Data:** 16/09/2026
- **xa11y fixado:** `https://github.com/xa11y/xa11y.git`
- **SHA:** `eeab1fcc4ba9020e7946fffb2307ae99f2440a59`
- **Versão observada:** `0.14.0`
- **Licença:** MIT
- **Requisito de Rust do upstream:** `1.88`; toolchain local instalado: `rustc 1.98.1`.

## Provas executadas

O checkout de auditoria está em `upstream/xa11y` e é ignorado no repositório principal. Com o linker local configurado:

```text
cargo test -p xa11y-core --locked
277 passed; 0 failed
doc-tests: 4 passed; 4 ignored

cargo run -p xa11y --locked -- --help
exit 0; CLI compilado e executado
```

O CLI expõe observação de apps/janelas/árvore, seletores semânticos, ações como `press`, `focus`, `toggle`, `select`, `set-value`, eventos e captura opcional. A documentação do crate declara backends AT-SPI2 no Linux, UI Automation no Windows e AXUIElement no macOS.

## Observação no ambiente atual

Sessão observada: Ubuntu 24.04, GNOME/Wayland, `XDG_SESSION_TYPE=wayland`, com apps X11 e Wayland coexistindo. O comando `xa11y apps` enumerou processos acessíveis como GNOME Shell, Chromium, Google Chrome, OpenCode, ChatGPT e Codex. `xa11y windows` retornou 24 janelas, incluindo Chrome, OpenCode, ChatGPT e janelas do desktop.

O comando `xa11y shell` retornou `No shell surfaces found`, portanto a superfície de shell precisa de uma capability/fallback próprio e não pode ser presumida como disponível.

## Apps de aceite

| Cenário | Resultado atual | Limite comprovado |
|---|---|---|
| Fixture controlada | Ainda não criada | T004 permanece aberto; T079 criará uma fixture com nós duplicados, stale refs, modal, senha e lista virtualizada. |
| VS Code | `code`/`code-insiders` não encontrados no PATH | Não há observação nem ação real em VS Code nesta máquina. Não declarar J02/J06 aprovado. |
| Discord | Processo Snap `Discord 1.0.158` existe, mas não apareceu em `xa11y apps` nem nas 24 janelas observadas | Login/árvore/acessibilidade não comprovados; não navegar nem inferir sucesso. |
| Apps acessíveis já abertos | Enumeração funciona para Chrome/OpenCode/ChatGPT/Codex | Isso prova apenas a sessão AT-SPI parcial, não a matriz completa de apps alvo. |

## Conclusão técnica provisória

- Rust consegue integrar a crate sem modificar o código do xa11y: os testes do core passaram e o CLI foi compilado.
- A abordagem de referência semântica é viável no Linux atual para alguns aplicativos.
- O relatório ainda não mede fixture, threading/bloqueio de cada app, chamadas de ação reais, VS Code, Discord, permissões, KDE/Wayland, X11, Windows ou macOS.
- A próxima prova segura é criar a fixture Vox e adaptar o cliente em worker; ações reais somente após revalidação de referência e política.
