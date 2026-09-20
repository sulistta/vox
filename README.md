# vocs — Jev + Vocaela nativos no Wayland

Este diretório contém o protótipo Python original (mantido como referência) e o
novo crate Rust. O binário Rust mantém uma sessão persistente
`RemoteDesktop + ScreenCast + PipeWire`, captura o último frame em memória,
consulta Vocaela para grounding e deixa Jev escolher uma ação de um conjunto
fechado por skill. Não há OCR, AT-SPI, EIS/libei, DOM ou áudio.

## Dependências

Além de Rust/Cargo, instale as bibliotecas usadas pelo build:

```bash
sudo apt install clang libclang-dev libpipewire-0.3-dev libwayland-dev libxkbcommon-dev libdbus-1-dev
```

O portal precisa de um backend Wayland com `RemoteDesktop` e `ScreenCast`, e o
servidor PipeWire deve estar ativo. `clang`/`libclang-dev` são usados pelo
`bindgen` para gerar os bindings do PipeWire durante a compilação.

Configure apenas a chave do Jev (não é gravada nem aparece nos logs):

```bash
cp .env.example .env
export TYPESAFE_API_KEY='...'
```

`VOCAELA_URL`, `TYPESAFE_BASE_URL`, `TYPESAFE_MODEL`, `JEV_MIN_CONFIDENCE`,
`MAX_STEPS` e `ACTION_SETTLE_MS` podem ser alterados por ambiente. O Vocaela é
iniciado externamente com `llama-server`; o Rust nunca reinicia nem incorpora o
servidor.

## Execução

Sem `--image`, o binário usa captura live e pede autorização do portal uma vez:

```bash
cargo run -- --goal "Search YouTube for rust programming"
cargo run -- --goal "Search YouTube for rust programming" --execute
```

`--execute` habilita os métodos D-Bus de input. Sem ele, grounding e debug são
executados, mas as ações ficam `Simulated`. Coordenadas de um screenshot estático
nunca podem ser usadas para input: `--image --execute` é rejeitado.

Para regressão offline do parser/grounding, use uma imagem estática (sem input):

```bash
cargo run -- --goal "Search YouTube for rust programming" --image screenshot.png
```

O modo live seleciona somente um monitor, mantém o stream PipeWire aberto e
chama `capture_latest()` antes de cada consulta Vocaela. O overlay/debug usa
dimensões em pixels; o pointer usa `logical_size` do stream quando disponível.
Após cada input há uma espera configurável, por padrão 300 ms.

Janelas sobrepostas, inclusive o terminal que iniciou o programa, continuam
fazendo parte da imagem capturada e podem ocultar o alvo completamente. O alvo
precisa estar ao menos parcialmente visível; mover/minimizar o terminal é uma
forma simples de evitar essa contaminação sem heurísticas específicas de app.

## Artefatos e logs

Cada execução cria `runs/<uuid>/` com o frame original, overlay com centro/bbox e
um JSON por passo. `tracing` registra decisão/confiança do Jev, instrução e
resposta do Vocaela, grounding, latências (`frame_acquire_ms`, `png_encode_ms`,
`base64_ms`, `vocaela_http_ms`, `vocaela_parse_ms`, `grounding_total_ms`), input
e erros — nunca a chave de API.

## Testes

```bash
cargo fmt --check
cargo test
cargo check
```

Os testes unitários não usam rede, portal, PipeWire ou `llama-server`. O teste
end-to-end live depende do desktop Wayland e das autorizações do portal.

Os arquivos `main.py`, `jev.py`, `vocaela.py`, `wayland_portal.py` e demais
scripts Python permanecem no repositório para comparação e regressão do MVP
anterior.
