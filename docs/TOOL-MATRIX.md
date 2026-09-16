# Matriz de efeitos das ferramentas

O broker é o único ponto que transforma uma intenção em efeito. A tabela
abaixo é a referência de T080: cada ferramenta possui pré-condição,
pós-condição, timeout e política de repetição. `unknown` exige observação ou
decisão do usuário; nunca há retry cego.

| Tool | Efeito | Pré-condição | Pós-condição observável | Timeout/cancelamento | Repetição |
|---|---|---|---|---|---|
| `desktop.list_windows` | read | backend de acessibilidade disponível | lista marcada como observada e fresca | chamada nativa; diagnóstico recuperável | segura, nova observação |
| `desktop.snapshot` | read | sessão AT-SPI/UIA/AX autorizada | snapshot limitado, refs opacas, redaction/truncation explícitas | limites de 512 nós/8 níveis | nova captura, nunca reutilizar stale |
| `desktop.query` | read | snapshot recebido e query bounded | refs que atendem papel/nome/estado/ação; ambiguidade permanece explícita | somente snapshot; nova captura antes de agir | segura sobre snapshot, não autoriza efeito |
| `desktop.wait_for` | read | backend nativo e query bounded | ref observada em nova captura ou `unknown` com timeout | polling bounded de até 30 s; cancelamento do run | nova observação, nunca retry de efeito |
| `desktop.act` | write/unknown | snapshot e ref ainda válidos; approval | dispatch separado da pós-condição; `verified:false` até reobservar | backend nativo; sem sucesso implícito | somente após reconciliação |
| `clipboard.read` | read | backend do SO disponível; nenhuma coleta contínua | texto limitado, MIME `text/plain`, backend e observação retornados | comando nativo bounded a 2 s; cancelamento do run | segura, nova leitura sob demanda |
| `clipboard.write` | write | texto bounded; approval hash-bound; backend do SO disponível | bytes escritos e backend retornados como `applied` somente após o processo terminar | comando nativo bounded a 2 s; cancelamento do run | não repetir sem decisão se o backend terminar em estado desconhecido |
| `files.read` | read | path dentro da raiz permitida | bytes/encoding/tamanho conferidos | limite de saída | segura |
| `files.search` | read | raiz existente e limite válido | candidatos dentro da raiz, truncation explícita | limite de entradas | segura |
| `files.list` | read | diretório existente | entradas e metadados bounded | limite de entradas | segura |
| `files.write` | write | approval hash-bound; pai permitido; destino ausente salvo overwrite | arquivo existe e tamanho confere | escrita temp + rename; sem worker pendurado | se destino existe, ler/reconciliar |
| `files.move` | write | approval hash-bound; origem identificada; destino sem conflito | destino existe e origem não existe | rename local ou cópia sincronizada explicitamente marcada cross-device | nunca repetir sem observar |
| `shell.exec` | arbitrary | argv explícito, cwd permitido, approval | exit code e output bounded; efeitos externos continuam `unknown` | timeout e cancel direto do filho | decisão humana após efeito |
| `process.list` | read | plataforma suporta enumeração | PID + identidade retornados | bounded | segura |
| `process.terminate` | external | PID + comando + start-time conferidos; approval | ausência observada em deadline vira `applied`; caso contrário `unknown` | TERM/taskkill; grupos privados somente para workers Vox | não repetir com PID reutilizado |
| `apps.resolve` | read | nome curto e bounded | caminho de executável presente no catálogo local; nenhum processo iniciado | consulta ao PATH; cancelamento não produz efeito | segura, nova consulta |
| `apps.launch` | external | programa/argv resolvidos; approval | spawn observado; janela ainda não é prova | processo separado | reconciliar antes de relançar |
| `paths.open` | external | path dentro da raiz; approval | launcher aceitou o caminho; app útil ainda pendente | launcher do SO | não repetir sem observar |

## Registro de efeito

Antes de executar uma ação, o caller deve manter `run_id`, `call_id`, hash dos
argumentos e approval específico. Depois deve registrar status, side effect,
verification e erro. O SQLite usa `idempotency_key` para não inserir o mesmo
resultado duas vezes, mas isso não torna a ação externa atômica.

## Limites conhecidos

- Execuções argv usam ambiente mínimo e grupo privado no Unix; ações externas
  continuam exigindo reconciliação.
- O adapter xa11y despacha ação nativa, mas só pode marcar sucesso após nova
  observação do estado-alvo.
- Clipboard não é observado em background. Linux usa `wl-paste`/`wl-copy` em
  Wayland e `xclip` em X11; macOS usa `pbpaste`/`pbcopy`; Windows usa
  PowerShell. O backend pode ser substituído por comando fixture em teste, sem
  persistir o conteúdo.
- `apps.launch` e `paths.open` informam spawn/aceite do launcher, não uma
  janela usável.
