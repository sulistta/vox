# Dependências, proveniência e licenças

Este inventário cobre as dependências diretas que entram na primeira fatia executável. Os lockfiles (`Cargo.lock` e `pnpm-lock.yaml`) são a fonte da resolução exata; upgrades devem alterar o lockfile em revisão separada e repetir a CI.

| Componente | Versão/pin | Uso | Licença/origem | Limite de uso |
|---|---:|---|---|---|
| eframe/egui | 0.36.2 | janela e renderização nativa | MIT/Apache-2.0; crates.io | não introduzir WebView/Tauri/Electron |
| xa11y | 0.14.0 | acessibilidade nativa | MIT; crates.io, auditado no SHA do relatório T004 | adapter Vox não altera o upstream |
| Pi auditado | `60e7e76bd7ea25cad1dd6f3f1ce0d18814a42759` | referência e candidatos de core | MIT; upstream documentado em T003 | nenhum CLI/TUI/extensão/config do usuário é incorporado |
| rusqlite | 0.37.0 | histórico local | MIT | SQLite local; sem envio automático |
| Node.js | >=22.19.0 | core privado durante desenvolvimento | licença própria/Node.js | empacotamento final ainda pendente |
| Keyring | libsecret `secret-tool` no Linux; Security.framework/Keychain no macOS; Windows Credential Manager no Windows | credencial do provider fora do SQLite | APIs nativas do SO | sem fallback em arquivo; aceitação operacional por SO ainda pendente |
| TypeScript | 5.9.3 | compilação do core/protocolo | Apache-2.0 | tipos não substituem validação Rust |

O produto ainda não está publicado como instalador, portanto este documento não declara assinatura, SBOM final ou compatibilidade de distribuição. T055/T056 precisam gerar esses artefatos antes de qualquer release.

## Processo de atualização

1. Atualizar uma dependência por vez, preservando o motivo e o impacto de licença.
2. Rodar `pnpm typecheck`, `pnpm test`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` e `cargo test --workspace`.
3. Repetir a auditoria de segurança e o teste de crash/cancelamento.
4. Conferir se o novo pacote não acessa credenciais, configurações do Pi ou efeitos fora do broker.
