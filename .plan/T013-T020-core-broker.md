# T013–T020 — core, broker, providers e runs

Data: 16/09/2026.

O core próprio TypeScript implementa handshake, sessão, streaming, cancelamento,
pedido de tool, correlação de resultado e eventos terminais. Os testes cobrem
o fluxo `mostrar janelas` (conversa → `desktop.list_windows` → broker →
`tool.result` → resposta) e uma interrupção durante streaming.

O broker Rust é o único caminho de efeitos no desktop. A política classifica leitura, escrita, externo e arbitrário; aprovações são hash-bound, expiram e são single-use. O runtime cobre leitura/busca/listagem/escrita/movimentação, clipboard bounded, argv sem shell, limite de saída, timeout, cancelamento, processos e abertura/lançamento com verificação explicitamente incompleta quando só houve spawn. O planejador textual só solicita ferramentas conservadoras; a classificação nunca concede autorização ao provider.

O teste de integração de broker também prova uma chamada `files.write` sem
approval (nenhum arquivo), uma aprovação exata (um arquivo) e replay sem nova
autorização. Erro de autorização/runtime é devolvido ao core como resultado
estruturado, sem deixar a chamada pendurada.

Há provider fake determinístico e provider OpenAI-compatible textual com SSE/JSON, validação de endpoint, redaction de erros e endpoint/modelo explícitos. O fallback JSON de tool valida formato e nome antes de qualquer broker. O core também aplica limites de prompt, saída acumulada, steps, duração e estagnação; unknown não vira sucesso.

Evidências: `pnpm typecheck`, `pnpm test`, `cargo test --workspace` e clippy passaram. Credencial comercial real, provider local dedicado, compactação de contexto e arbitragem entre sessões ainda não foram afirmados como concluídos.
