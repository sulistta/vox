# Desenvolvimento local

O Vox é uma janela nativa Rust/egui com um core TypeScript privado supervisionado por NDJSON. O core não é o CLI do Pi e não lê a configuração do Pi instalado pelo usuário.

## Verificação rápida

```sh
pnpm install
pnpm typecheck
pnpm test
pnpm build
cargo fmt --all -- --check
cargo test --workspace
cargo build -p vox-desktop
```

Para montar o bundle de desenvolvimento com o binário Node usado pelo core:

```sh
pnpm package:dev
./dist/vox-dev/run.sh
```

O bundle é autocontido para o runtime de desenvolvimento, mas ainda não é um
release assinado: não fecha atualização entre versões reais, notices, ou a
matriz multiplataforma de T055/T056/T082. O recorte Linux inclui um launcher
instalável e um SBOM dev verificável.

```sh
pnpm bundle:validate dist/vox-dev
pnpm package:linux /tmp/vox-linux-dev.tar.gz
```

O workflow de CI repete os gates do bundle no Ubuntu e publica o tarball Linux
e seu checksum como artefato de desenvolvimento.

O launcher instalado fica em `~/.local/vox/bin/vox` por padrão; o prefixo pode
ser alterado com `VOX_INSTALL_PREFIX` ou `--prefix`. `linux/uninstall.sh`
preserva os dados por padrão e só os remove com `--remove-data`.

Na máquina de referência Linux sem toolchain de sistema, a validação local usou Rust 1.98.1 e um toolchain GCC/binutils extraído em `/tmp/vox-toolchain/root`; a CI usa toolchains nativos de cada runner. Não copie chaves ou tokens para os logs.

## Executar a janela

Depois de `pnpm build` e `cargo build -p vox-desktop`:

```sh
VOX_AGENT_ENTRY="$PWD/packages/agent-core/dist/main.js" \
  ./target/debug/vox-desktop
```

`VOX_PROVIDER=fake` é o padrão seguro e textual: ele responde à conversa, mas não propõe nem executa ferramentas. Para testar a fixture determinística de ferramentas, use somente em testes `VOX_PROVIDER=fake-tools`. Um endpoint OpenAI-compatible exige `VOX_PROVIDER=openai-compatible`, `VOX_PROVIDER_BASE_URL` e `VOX_PROVIDER_MODEL`; `VOX_PROVIDER_API_KEY` fica somente no processo e nunca é impresso. Pela janela, salvar endpoint e modelo em Preferências ativa esse provider no próximo core privado ocioso.

Para verificar uma conta OpenAI-compatible configurada no ambiente, após
`pnpm build` rode `pnpm smoke:provider`; para testar cancelamento, rode
`VOX_PROVIDER_SMOKE_MODE=cancel pnpm smoke:provider`. O script exige que
`VOX_PROVIDER_BASE_URL`, `VOX_PROVIDER_MODEL` e `VOX_PROVIDER_API_KEY` já
estejam disponíveis no processo. Ele bloqueia todas as tools, não espelha
payload/resposta/stderr e imprime somente um resumo redigido. Não salve a chave
em `.env` versionado, SQLite ou na linha de comando.

Para usar o keyring no Linux, configure `VOX_PROVIDER_ACCOUNT` ou informe a
conta em Preferências e grave a chave pelo botão explícito. Endpoint e modelo
também podem ser editados nas Preferências; quando não há run ativo, a mudança
reinicia somente o core privado e mantém a janela/sessão. O Vox usa
`secret-tool`/libsecret, não cria arquivo de fallback e injeta a chave somente
no próximo processo privado do core. Se `secret-tool` não estiver instalado,
a indisponibilidade deve ser corrigida no ambiente; não coloque a chave no
SQLite ou em logs. Em macOS e Windows, o build usa os backends nativos de
Keychain/Credential Manager; a aceitação desses backends depende da execução na
matriz CI e nas máquinas de referência.

`VOX_XA11Y_BIN` pode apontar para um binário xa11y apenas como fallback operacional. A ferramenta de janelas tenta primeiro o adapter Rust nativo; se a ponte de acessibilidade não estiver disponível, o resultado é uma falha factual e recuperável, não uma fixture apresentada como observação real.

No Linux, a árvore da própria janela egui é publicada pelo AccessKit quando o
serviço de leitor de tela do sistema está ativo (`org.a11y.Status`
`ScreenReaderEnabled`). Isso evita expor a árvore continuamente em uma sessão
sem acessibilidade em uso. O smoke real não muda essa preferência; com uma
sessão já ativa, execute
`VOX_TEST_A11Y=1 cargo test -p vox-desktop --test native_accessibility -- --ignored --nocapture`
para verificar labels, menu e Preferências da janela Vox.

Em **Preferências → Pastas permitidas**, informe uma pasta por linha. O
runtime canoniza essas pastas e rejeita caminhos, traversal e symlinks que
saiam delas. Na primeira execução, a lista usa Desktop, Documents, Downloads e
o checkout atual quando ele estiver dentro da pasta pessoal; ela nunca libera
a pasta pessoal inteira. A alteração fica salva no SQLite e só troca o broker
depois que a tarefa em andamento termina, para que uma operação não mude de
escopo no meio da execução.

O histórico usa SQLite persistente no diretório de dados da plataforma. Para
um smoke isolado, defina `VOX_DATA_DIR` para um diretório temporário; as
migrações não reconstroem approvals antigos e a exportação redige valores com
aparência de segredo. O schema v4 guarda o rascunho separadamente por sessão,
redige-o antes da escrita, restaura a última sessão válida no próximo início e
o exclui da exportação. O backup SQLite pode conter apenas a versão já
redigida do rascunho.

Resultados de ferramentas concluídas também são persistidos como contexto
interno redigido, sem aparecer como bolhas da conversa. Isso permite que o
turno seguinte recupere uma observação ou efeito anterior; a numeração do
histórico vem do SQLite, e não da quantidade de bolhas visíveis.

## Limites atuais

O caminho executável validado nesta máquina é Linux/Wayland. Windows, macOS, GNOME/KDE/X11 e instalação limpa têm jobs e documentação de validação, mas não devem ser declarados aprovados até que a execução real do ambiente seja registrada nos artefatos de T005–T008 e T047–T056.
