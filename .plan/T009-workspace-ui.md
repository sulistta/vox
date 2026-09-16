# T009 — workspace e UI mínima

Data: 16/09/2026.

Entregues:

- Cargo workspace Rust com `apps/desktop` e crates separados para protocolo, política, broker, ferramentas, acessibilidade, persistência e supervisor;
- workspace pnpm com protocolo, adapters de provider e core TypeScript;
- lockfiles `Cargo.lock` e `pnpm-lock.yaml`;
- janela nativa eframe/egui, sem Tauri, Electron ou WebView;
- UI com conversa, streaming, modo compacto, Parar, aprovação pendente, menu, rascunho e modelo visível;
- testes do reducer de apresentação e build local Linux.

## Evidência

`cargo build -p vox-desktop` passou em Linux/Wayland. O processo `target/debug/vox-desktop` permaneceu vivo durante a probe nativa; a janela não apareceu na enumeração AT-SPI desta sessão, portanto o aceite multiplataforma e a prova visual completa continuam pendentes em T005–T009/T074.
