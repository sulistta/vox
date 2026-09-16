# Changelog

## Unreleased — 2026-09-16

### Adicionado

- workspace Rust/TypeScript com protocolo NDJSON v1 e fixtures cross-language;
- core textual privado, provider fake/OpenAI-compatible e fluxo de cancelamento;
- broker Rust com aprovação hash-bound, expiração e single-use;
- runtime de arquivos/processos/launch com limites, redaction e resultados
  `unknown` explícitos;
- adapter xa11y nativo, snapshots limitados e fixture semântica adversarial;
- SQLite persistente com migração v1→v2, recovery, idempotência e exportação
  redigida;
- janela egui com histórico, streaming, compacto, Parar, approval preview,
  preferências e troca de sessões;
- CI, matriz de QA/efeitos, documentação de release e suporte.

### Limitações conhecidas

- o bundle atual é de desenvolvimento e ainda exige Node.js;
- Linux Ubuntu/GNOME/Wayland foi observado; X11/KDE, Windows e macOS ainda
  precisam de aceite de desktop real;
- voz, keyring nativo, pós-condição de ações em apps reais e beta de uso diário
  não foram declarados concluídos;
- assinatura, SBOM, instaladores e publicação v1 aguardam os gates do plano.
