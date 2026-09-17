# RFC de voz em português

A v1 deve manter texto como caminho completo e exigir push-to-talk explícito. A escolha final do STT permanece aberta até existir corpus pt-BR, medição de latência e teste de permissão nos três sistemas.

## Critérios

- transcrição local sem envio implícito;
- licença e redistribuição compatíveis com o instalador;
- início/fim observável, limite de duração e cancelamento real;
- nomes de aplicativos, caminhos e acentos pt-BR no corpus;
- nenhuma captura contínua ou wake word na v1;
- transcrição editável antes de virar `turn.start`.

## Contrato determinístico já implementado

O pacote `@vox/agent-core` agora contém `VoiceController`, um contrato
independente da plataforma para a parte que pode ser validada sem fingir um
microfone real. Ele mantém os estados `idle`, `recording`, `transcribing`,
`review`, `failed` e `cancelled`; limita a duração; aceita cancelamento;
recusa áudio vazio; classifica falhas de permissão; e só entrega o texto após
uma etapa explícita de revisão/edição. O consumidor deve usar o texto retornado
por `accept()` como conteúdo de um `turn.start` com `source: "voice"`.

Os testes cobrem revisão antes do turno, áudio/transcrição vazios,
cancelamento, expiração de duração e a fronteira de captura bounded do crate
[`vox-audio-capture`](./VOICE-CAPTURE.md). O `UnavailableTranscriber` marca a
ausência de STT como `TRANSCRIPTION_UNAVAILABLE`, sem permitir que bytes de
áudio virem uma mensagem. A janela consulta o backend Linux por PipeWire/ALSA
sem iniciar uma gravação e deixa a voz explicitamente desabilitada até haver
uma transcrição conectada. A implementação nativa já possui encerramento por
key-up/perda de foco, watchdog e limite de memória. Ainda não seleciona
dispositivo por uma UI, solicita permissões da sessão, executa STT ou envia
áudio: essas partes continuam dependentes da integração nativa completa e de
uma medição com hardware autorizada.

## Comparação a executar

Comparar uma engine local baseada em Whisper com um serviço remoto configurável e um modo desabilitado. Registrar WER/CER, latência p50/p95, memória, tamanho do pacote, ruído e comportamento quando microfone/permissão falhar. O contrato foi testado localmente, mas nenhum resultado de engine, hardware ou serviço foi simulado nesta revisão.
