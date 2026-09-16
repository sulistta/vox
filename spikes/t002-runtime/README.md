# T002 — Spike de runtime Rust/TypeScript

Este spike compara um core TypeScript em processo supervisionado com uma função embutida no processo hospedeiro. Ele não usa o executável Pi e mede apenas o contrato mínimo: handshake, streaming, cancelamento e encerramento.

Execute:

```bash
node spikes/t002-runtime/run.mjs
```

O resultado registra versão do Node, tamanho do worker, tempo até handshake, primeiro evento, terminal normal e cancelado. O cenário de crash usa `mode: "crash"` no worker e é mantido como prova separada durante a decisão A01.

## Resultado inicial

O spike TypeScript é reproduzível no Node `22.23.2` desta máquina. O processo separado fornece fronteira de crash, stdout protocolado e cancelamento observável; o modo embutido reduz o custo de startup, mas não isola uma falha do runtime. A medição de Rust/eframe e a comparação de build/artefato nos três sistemas dependem de T009 e T002 completo.

## Decisão provisória A01

Manter o **core TypeScript em processo privado supervisionado** para a primeira implementação. A separação é necessária para recuperar o core sem congelar a GUI e para dar ao cancelamento um canal explícito. O processo não será o CLI do Pi: será um entrypoint Vox, com protocolo privado, sem leitura de configuração/extensões do Pi.

Essa decisão ainda não fecha o aceite de T002: falta medir o binário Rust/eframe, tamanho de distribuição, crash/cancelamento ponta a ponta e builds mínimos nos três sistemas.
