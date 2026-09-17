# T013–T020 — core, broker, providers e runs

Data: 16/09/2026.

O core próprio TypeScript implementa handshake, sessão, streaming, cancelamento,
pedido de tool, correlação de resultado e eventos terminais. Os testes cobrem
o fluxo `mostrar janelas` (conversa → `desktop.list_windows` → broker →
`tool.result` → resposta) e uma interrupção durante streaming.

O broker Rust é o único caminho de efeitos no desktop. A política classifica leitura, escrita, externo e arbitrário; aprovações são hash-bound, expiram e são single-use. O runtime cobre leitura/busca/listagem/escrita/movimentação, clipboard bounded, argv sem shell, limite de saída, timeout, cancelamento, processos e abertura/lançamento com verificação explicitamente incompleta quando só houve spawn. O provider recebe catálogo e contexto redigido, devolve uma decisão JSON estrita, e o core só encaminha chamadas validadas ao broker; essa decisão nunca concede autorização.

O teste de integração de broker também prova uma chamada `files.write` sem
approval (nenhum arquivo), uma aprovação exata (um arquivo) e replay sem nova
autorização. Erro de autorização/runtime é devolvido ao core como resultado
estruturado, sem deixar a chamada pendurada.

Há provider fake seguro para demonstração, fixture `fake-tools` exclusiva dos testes e provider OpenAI-compatible textual com SSE/JSON, validação de endpoint, redaction de erros e endpoint/modelo explícitos. O parser de decisão JSON valida formato, nome e argumentos antes de qualquer broker. O core também aplica limites de prompt, saída acumulada, steps, duração e estagnação; unknown não vira sucesso. Cada solicitação efetivamente enviada ao provider gera `model.requested` com a mesma versão redigida que a janela exibe sob demanda.

Evidências: `pnpm typecheck`, `pnpm test`, `cargo test --workspace` e clippy passaram. A compactação de contexto e a arbitragem entre sessões possuem recorte local; credencial comercial real, provider local dedicado e a avaliação de uso real ainda não foram afirmados como concluídos.
