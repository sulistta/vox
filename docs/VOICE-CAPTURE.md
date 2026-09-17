# Captura nativa de voz

O crate `vox-audio-capture` fornece a fronteira nativa de captura sem fazer
shell interpolation e sem persistir áudio. Ele trabalha com PCM `s16`,
16 kHz, mono por padrão, impõe limite de duração e de bytes e calcula um nível
de pico apenas para feedback transitório.

No Linux, a seleção prefere `pw-record`/PipeWire e usa `arecord`/ALSA como
fallback. O comando recebe argumentos fixos e escreve o áudio em stdout; não
há arquivo temporário de gravação. O processo fica em um grupo privado para
que Parar, cancelamento e perda de foco não deixem um gravador órfão.

`key_down()` inicia, `key_up()` encerra e devolve um `CaptureResult`, enquanto
`focus_lost()` cancela e descarta os bytes. O watchdog encerra a captura no
limite máximo. Saída acima do limite continua sendo drenada, mas o resultado é
marcado como truncado para evitar deadlock de pipe e crescimento ilimitado de
memória.

O backend tem testes determinísticos de captura bounded, pico de nível,
cancelamento por foco, duplicidade e limite de bytes. A janela detecta o
backend disponível sem abrir o microfone e mostra essa informação ao usuário.
Ela mantém o controle de voz desabilitado enquanto não houver uma engine STT
conectada, porque áudio bruto não pode virar um `turn.start`. Ainda falta
selecionar/empacotar essa engine, solicitar a permissão da sessão e ligar a
transcrição revisável ao turno. Nenhuma captura de microfone real é feita
automaticamente pelos testes.

## Verificação manual por plataforma

Antes de promover a voz, registrar dispositivo, permissão concedida/revogada,
key-down/key-up, suspensão/perda de foco, bytes/duração e resultado STT. A
enumeração Linux pode ser consultada com `arecord -l` ou pelo painel PipeWire;
isso não substitui uma gravação explícita autorizada nem uma avaliação pt-BR.
