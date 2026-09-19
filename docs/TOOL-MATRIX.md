# Matriz de ferramentas

O envio explícito de uma tarefa cria um `TaskGrant` com ferramentas inferidas do pedido. O broker só executa uma chamada se o grant pertencer ao mesmo run, estiver ativo e contiver a ferramenta. O runtime continua sendo a autoridade para raízes de arquivos, argumentos, identidade de processos e efeitos.

| Ferramenta | Pré-condições além do grant | Verificação | Repetição |
|---|---|---|---|
| `desktop.list_windows` | bridge de acessibilidade disponível | lista recente de janelas/PIDs | nova observação |
| `desktop.snapshot` / `desktop.query` / `desktop.wait_for` | PID já observado quando informado; snapshot atual | referências e evidência nativa | nunca reutilizar ref vencida |
| `desktop.act` | ref emitida pelo broker; campo não sensível | pós-condição observada, ou `unknown` | nunca repetir automaticamente |
| `apps.resolve` / `apps.launch` | `apps.launch` só aceita executável resolvido no mesmo grant | spawn não prova janela utilizável | observar antes de relançar |
| `process.list` / `process.terminate` | término requer PID, comando e início observados no mesmo grant | ausência do processo | nunca reutilizar PID |
| `files.read` / `files.search` / `files.list` | caminho dentro de raiz permitida | bytes/listagem bounded | segura sob nova leitura |
| `files.write` / `files.move` | origem/destino dentro de raiz; sobrescrita explícita | tamanho, existência e origem/destino | reconciliar antes de nova ação |
| `clipboard.read` / `clipboard.write` | backend do SO disponível | backend e bytes retornados | não repetir após estado incerto |
| `paths.open` | caminho dentro de raiz | launcher aceitou o alvo | observar aplicativo depois |
| `shell.exec` | pedido menciona terminal/comando; argv e cwd passam pelo runtime | exit code e saída bounded | efeitos externos permanecem `unknown` até evidência |

Uma chamada abaixo da confiança exigida faz Jev escolher reobservação ou replanejamento. A confiança nunca amplia o grant. O journal registra a intenção antes do efeito; uma queda converte intenção pendente em `unknown`.
