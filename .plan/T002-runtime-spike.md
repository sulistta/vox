# T002 — resultado do spike de runtime

Data: 16/09/2026. O spike reproduzível está em [`spikes/t002-runtime`](../spikes/t002-runtime).

## Medição local

Com Node.js `v22.23.2`, o comando `pnpm t002` executou processo separado, cancelamento, crash e runtime embutido:

| Caso | Resultado observado |
|---|---:|
| processo separado — initialize | aproximadamente 51–66 ms |
| processo separado — turno terminal | aproximadamente 73–88 ms |
| processo separado — cancelamento | aproximadamente 60 ms |
| processo separado — crash | exit code 17, aproximadamente 57 ms |
| runtime embutido | aproximadamente 5,4 ms |

O processo separado fornece isolamento de crash e permite manter o core TypeScript privado, enquanto o embedding reduz latência mas aumenta o acoplamento e não representa ainda um binário distribuível. A decisão provisória A01 é usar processo privado supervisionado para a primeira fatia; o embedding fica como otimização posterior.

## Limite de aceite

O aceite completo de T002 permanece aberto: não há build medido do runtime empacotado nos três sistemas nem tamanho final de instalador. Esses resultados não são apresentados como prova de distribuição final.
