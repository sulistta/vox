# Modelo de segurança operacional

O modelo é: o core interpreta intenção e transmite pedidos; o broker Rust valida ferramenta, argumentos, escopo e autorização; somente o runtime Rust produz efeitos. Snapshots de acessibilidade são observações limitadas e nunca são tratados como autorização.

Snapshots são emitidos pelo broker, vinculados ao run e expiram rapidamente;
o modelo devolve somente o identificador opaco do snapshot. Uma ação consome a
observação e a API nativa revalida o elemento. O Vox não preenche campos de
acessibilidade reconhecidos como credencial, mesmo quando o modelo tenta
propor a ação. A regra usa estado semântico e rótulos completos comuns em
português/inglês, como senha, segredo, credencial, token, PIN e chave; o valor
é redigido antes de cruzar a fronteira do modelo.

- leituras são limitadas por raiz, tamanho e paginação;
- escrita/movimentação, shell, lançamento e término de processo exigem aprovação vinculada ao hash exato dos argumentos;
- aprovações expiram e são consumidas uma vez;
- resultados `unknown` não são repetidos automaticamente;
- stdout/stderr e eventos passam por limites para não travar IPC;
- valores com aparência de credencial são redigidos antes de persistir/exportar;
- rascunhos não enviados são redigidos antes de entrar em `session_drafts` e
  não entram no transcript, contexto do modelo ou exportação; ao enviar, o
  turno também cruza a fronteira do modelo somente depois da redação, e o
  backup guarda apenas a versão redigida;
- a parada aborta o core e bloqueia novas ações do run, sem prometer rollback de efeito já incerto;
- conteúdo da interface é texto/Markdown restrito; HTML e comandos não são executados ao clicar.

O adaptador de T022 usa o keyring nativo disponível: libsecret/`secret-tool` no
Linux, Security.framework/Keychain no macOS e Windows Credential Manager no
Windows. O Linux recebe o segredo por stdin quando grava; os outros dois
backends usam APIs nativas e não colocam o segredo em argumentos de processo.
Uma cópia só é injetada no ambiente do processo privado do core quando
`VOX_PROVIDER_ACCOUNT` é definido. O SQLite, exportação e logs não recebem a
chave. Não existe fallback silencioso para arquivo; a aceitação operacional de
macOS/Windows ainda precisa ocorrer nas máquinas de referência.

A tela de preferências aceita a conta e um campo mascarado somente para a
gravação explícita; o campo é limpo após sucesso e a aplicação ocorre no
próximo processo privado do core (imediatamente quando ocioso, ou após o run
terminar), sem hot-reload implícito durante uma execução.

“Ver atividade” mostra somente a versão redigida do pedido que saiu para o
provider. Antes de renderizar detalhes de uma aprovação, a janela reaplica
redação recursiva a chaves de credencial em inglês e português, incluindo
`password`, `secret`, `token`, `senha`, `segredo`, `credencial` e
`chave`. Tokens compatíveis com OpenAI/OpenRouter colados sem rótulo (`sk-…`)
também são redigidos antes de persistência, atividade e envio ao modelo; o
texto bruto não é um mecanismo de autorização.

Se a aplicação for encerrada com um rascunho, a última sessão pode restaurar a
versão já redigida desse texto. Valores que se pareçam com credenciais em campos
de endpoint, modelo ou conta são recusados como configuração e preferências
legadas com esse conteúdo são removidas ao abrir o SQLite. Diagnósticos do core
em stderr são suprimidos pelo supervisor para não transformar um erro remoto em
vazamento para a janela ou log.
