# T021–T025 — dados e acessibilidade

Data: 16/09/2026.

`vox-session-store` usa SQLite local com migração v1→v4, mensagens, resultados
de ferramentas redigidos para contexto interno, runs, preferences, journal de
efeitos idempotente, recuperação de runs abertos, listagem com
busca/paginação/estado, exportação e exclusão protegida durante run ativo. A
tabela separada `session_drafts` é vinculada por foreign key com cascade,
guarda apenas o rascunho já redigido e não faz parte da exportação; o backup
SQLite pode conter somente essa versão redigida. A janela usa um
arquivo persistente no diretório de dados da plataforma (ou `VOX_DATA_DIR` no
smoke). Mensagens, efeitos e exportação redigem padrões de credencial; não
existe fallback plaintext para chave e approvals não são reconstituídos depois
do reinício. Approvals persistem apenas auditoria com hash/escopo/expiração e
ficam invalidados na recuperação; corrupção do arquivo recebe erro
classificado, e a retenção só remove sessões terminais, podendo proteger a
sessão atual. Tema e nível da janela são persistidos como preferências não
secretas.

`vox-desktop-access` integra o crate `xa11y` 0.14.0 sem modificar upstream. O adapter lista janelas reais, captura snapshots textuais com refs opacas, limite de 512 nós/8 níveis, foco, ações/capabilities e redaction de campos sensíveis. Texto de nó e nomes de janela têm limite e ids longos recebem referência opaca com hash, evitando que uma única árvore estoure o IPC. A referência usa o id estável da plataforma quando disponível, escopado pelo app para não mudar a cada enumeração; árvores inacessíveis de um Electron/Chromium não ocultam os outros apps. O broker guarda o snapshot emitido por run durante uma janela curta: consulta e ação usam apenas seu `snapshot_id`, e uma ação consome a referência antes de a API nativa re-resolvê-la. Campos reconhecidos por estado ou rótulo completo em português/inglês — senha, segredo, credencial, token, PIN e chave — têm valor redigido e recusam `set_value`; a ponte repete a detecção depois de re-resolver o elemento nativo. `desktop.snapshot` e `desktop.wait_for` aceitam `app_pid` opcional, validado pelo broker, para observar somente o processo que já foi identificado. A ação nativa declara quando a pós-condição ainda precisa ser observada. `MockDesktop` cobre ambiguidade, stale ref, toggle e campo sensível para testes determinísticos.

Probe real local: `cargo run -p vox-desktop-access --example native_probe` enumerou janelas do GNOME, Codex, Discord, terminal e navegador na sessão Wayland, com ids de janela repetíveis entre duas observações. A saída publica apenas contagens, papéis, estados, bounds e ids opacos, nunca o texto cru de janelas externas.

O teste ignorado `crates/desktop-access/tests/native_fixture.rs` inicia uma
fixture GTK controlada e prova no AT-SPI real Linux a enumeração direcionada
por PID, nomes duplicados sem resolução arbitrária, senha e token em português
redigidos e protegidos contra `set_value`, `press` cuja pós-condição continua `unknown`, `wait_for`,
recriação de item com recusa da ref antiga, snapshot truncado por limite e
diálogo. Ele passou nesta sessão em cerca de dezenove segundos. O teste ignorado
`apps/desktop/tests/native_accessibility.rs` inicia o Vox em modo fake seguro
e verifica o rótulo associado do compositor, menu e Preferências sem ler
conteúdo do usuário. Ele exige
que o serviço de leitor de tela já esteja ativo, porque o AccessKit publica a
árvore da janela egui nessa condição; o teste não altera a preferência do SO.

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
