# T011 — IPC e supervisor

Data: 16/09/2026.

O supervisor usa stdin/stdout/stderr privados, valida cada linha, entrega diagnóstico tanto para stdout inválido quanto quando stdout fecha inesperadamente, mantém requisições pendentes com idade, expira pendências sob demanda, envia heartbeat periódico e encerra o filho no `Drop`. O core envia somente eventos NDJSON válidos; a resposta de tool é correlacionada por `run_id`, `call_id` e nome da ferramenta.

Evidências:

- `crates/supervisor/tests/supervisor.rs` valida handshake, sessão, streaming, terminal, EOF inesperado e stdout inválido;
- o teste de tool envia um `call_id` errado, confirma o erro de correlação e depois completa a chamada correta;
- o teste de saída inesperada usa um core que encerra sem protocolo e confirma diagnóstico sem bloquear o caller;
- `apps/desktop` reconcilia efeitos `pending` do run ativo como `unknown` antes de reiniciar o core, para que uma queda em processo vivo não dependa de reabrir o SQLite;
- `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace` passaram.

O heartbeat agora é um contrato versionado e a janela marca o core como sem
resposta após seis segundos sem retorno; pendências sem progresso por doze
segundos acionam o watchdog de reinício. A janela tenta no máximo três
reinícios após EOF/IPC inválido, interrompe o run que ficou sem core e não
reporta completion; reconciliação de efeitos desconhecidos e persistência do
crash-loop ainda precisam de integração de produto antes do aceite total.
