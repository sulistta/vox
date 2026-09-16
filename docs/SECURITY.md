# Modelo de segurança operacional

O modelo é: o core interpreta intenção e transmite pedidos; o broker Rust valida ferramenta, argumentos, escopo e autorização; somente o runtime Rust produz efeitos. Snapshots de acessibilidade são observações limitadas e nunca são tratados como autorização.

- leituras são limitadas por raiz, tamanho e paginação;
- escrita/movimentação, shell, lançamento e término de processo exigem aprovação vinculada ao hash exato dos argumentos;
- aprovações expiram e são consumidas uma vez;
- resultados `unknown` não são repetidos automaticamente;
- stdout/stderr e eventos passam por limites para não travar IPC;
- valores com aparência de credencial são redigidos antes de persistir/exportar;
- a parada aborta o core e bloqueia novas ações do run, sem prometer rollback de efeito já incerto;
- conteúdo da interface é texto/Markdown restrito; HTML e comandos não são executados ao clicar.

O adaptador de T022 usa o keyring nativo disponível (libsecret/`secret-tool` no
Linux), recebe o segredo por stdin quando grava e injeta uma cópia somente no
ambiente do processo privado do core quando `VOX_PROVIDER_ACCOUNT` é definido.
O SQLite, exportação e logs não recebem a chave. Não existe fallback silencioso
para arquivo. Security.framework no macOS e Windows Credential Manager ainda
precisam de implementação/aceite em suas máquinas de referência.

A tela de preferências aceita a conta e um campo mascarado somente para a
gravação explícita; o campo é limpo após sucesso e a aplicação fica limitada ao
próximo processo do core, sem hot-reload implícito.
