# T010 — contratos compartilhados

Data: 16/09/2026.

O contrato NDJSON v1 existe em `schemas/ipc.schema.json`, `packages/protocol/src/index.ts` e `crates/protocol/src/lib.rs`. Ambos rejeitam JSON inválido, tipo desconhecido, frames acima de 1 MiB e versão incompatível; `tool.result` exige `call_id` e o broker recebe `status`, `side_effect`, verificação e dados limitados.

As fixtures comuns estão em `tests/contracts/valid-turn.json`, `tests/contracts/valid-tool-result.json` e `tests/contracts/invalid-version.json`. Os testes TS e Rust leem as mesmas fixtures. Evidência: `pnpm test` e `cargo test --workspace` passaram.
