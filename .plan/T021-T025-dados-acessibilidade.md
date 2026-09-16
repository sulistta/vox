# T021–T025 — dados e acessibilidade

Data: 16/09/2026.

`vox-session-store` usa SQLite local com migração v1→v3, mensagens, runs,
preferences, journal de efeitos idempotente, recuperação de runs abertos,
listagem com busca/paginação/estado, exportação e exclusão protegida durante run ativo. A janela usa um
arquivo persistente no diretório de dados da plataforma (ou `VOX_DATA_DIR` no
smoke). Mensagens, efeitos e exportação redigem padrões de credencial; não
existe fallback plaintext para chave e approvals não são reconstituídos depois
do reinício. Approvals persistem apenas auditoria com hash/escopo/expiração e
ficam invalidados na recuperação; corrupção do arquivo recebe erro
classificado, e a retenção só remove sessões terminais, podendo proteger a
sessão atual. Tema e nível da janela são persistidos como preferências não
secretas.

`vox-desktop-access` integra o crate `xa11y` 0.14.0 sem modificar upstream. O adapter lista janelas reais, captura snapshots textuais com refs opacas, limite de 512 nós/8 níveis, foco, ações/capabilities e redaction de nomes sensíveis. A ação nativa re-resolve a referência e declara quando a pós-condição ainda precisa ser observada. `MockDesktop` cobre ambiguidade, stale ref, toggle e segredo para testes determinísticos.

Probe real local: `cargo run -p vox-desktop-access --example native_probe` enumerou GNOME Shell, Chromium, Google Chrome, OpenCode, Codex e terminal na sessão Wayland. Nenhum dado da fixture foi usado para essa observação.

O adaptador `vox-secrets` usa libsecret/`secret-tool` no Linux, Security.framework
no macOS e Windows Credential Manager no Windows, sem fallback em arquivo, e
injeta a chave apenas no ambiente do processo privado do core quando
`VOX_PROVIDER_ACCOUNT` está configurado. Fixture cobre round-trip, ausência,
limite e conta inválida. A aceitação operacional de macOS/Windows e a validação
visual completa do formulário de credencial ainda estão pendentes; não se deve tratar variável de
ambiente como armazenamento persistente.

A compilação condicional do crate passou em `x86_64-pc-windows-gnu` e
`x86_64-apple-darwin` com `cargo check`; isso verifica a fronteira de FFI, não
substitui o teste com Credential Manager/Keychain em uma sessão real.
