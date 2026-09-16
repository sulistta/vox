# T003 — Auditoria e fixação do upstream Pi

- **Status:** concluída para a decisão de proveniência e fork seletivo
- **Responsável:** Codex
- **Data:** 16/09/2026
- **Upstream fixado:** `https://github.com/earendil-works/pi.git`
- **SHA:** `60e7e76bd7ea25cad1dd6f3f1ce0d18814a42759`
- **Título do commit:** `fix(coding-agent): surface clipboard backend failures`
- **Versão do monorepo:** `0.0.3`
- **Versão dos pacotes de agente/AI/protocolo:** `0.85.1`
- **Licença declarada:** MIT, conferida no `LICENSE` do upstream e nos manifests dos pacotes candidatos.

## Evidência de resolução

Em 16/09/2026, `git ls-remote --symref` apontou o mesmo `HEAD` para os endereços consultados:

```text
https://github.com/earendil-works/pi.git  60e7e76bd7ea25cad1dd6f3f1ce0d18814a42759
https://github.com/badlogic/pi-mono.git    60e7e76bd7ea25cad1dd6f3f1ce0d18814a42759
```

O checkout de auditoria foi feito em `upstream/pi` e é ignorado pelo repositório principal para evitar um repositório Git aninhado acidental. A URL, SHA, inventário e estratégia abaixo são a fonte reproduzível.

## Inventário dos pacotes

| Pacote | Papel observado | Dependências diretas | Decisão Vox |
|---|---|---:|---|
| `@earendil-works/pi-agent-core` | loop, estado, mensagens, tools, streaming, sessões | 7 | Candidato principal a extração seletiva |
| `@earendil-works/pi-ai` | tipos, providers, APIs e streaming de modelos | 10 | Candidato para adapter textual, sem CLI/imagens por padrão |
| `@earendil-works/pi-protocol` | codec/framing/protocolo remoto | 2 | Referência de validação; IPC Vox permanece NDJSON próprio |
| `@earendil-works/pi-session-backend-sqlite-node` | backend de sessão SQLite | 2 | Referência; persistência Vox terá contrato próprio |
| `@earendil-works/pi-server` / `pi-client` | sessão remota e transporte | 3 / 2 | Não entram na primeira extração |
| `@earendil-works/pi-coding-agent` | produto de coding agent, CLI e integração de terminal | 19 | Excluído do produto Vox |
| `@earendil-works/pi-tui` | interface terminal | 2 | Excluído do produto Vox |

Métricas observadas no checkout de auditoria: aproximadamente 43 MiB de árvore de trabalho e 15,56 MiB de objetos Git empacotados. O package `pi-agent-core` declara Node `>=22.19.0`; o ambiente atual possui Node `22.23.2`, mas isso não substitui a criação do runtime próprio.

## Candidatos a reaproveitamento

- `packages/agent/src/agent-loop.ts`: sequência de turno, streaming, tool calls, abort e hooks de execução.
- `packages/agent/src/agent.ts` e `packages/agent/src/types.ts`: estado e contratos de agente, depois de retirar pressupostos de coding agent.
- `packages/agent/src/stream-fn.ts`: fronteira explícita de stream/provider.
- `packages/agent/src/harness/session/*`: ideias e testes de sessão; somente módulos necessários, sem descoberta automática de extensões/configuração do Pi.
- `packages/ai/src/providers/*`, `src/api/*` e `src/utils/*`: adapters textuais, parsing, retry e normalização, sujeitos à revisão de licença/dependências e ao contrato do broker.
- `packages/protocol/src/framing.ts` e `src/codec.ts`: referência para framing/limites; não são usados para expor porta pública.

## Exclusões obrigatórias

- `packages/coding-agent`, `packages/tui`, binários/entradas CLI, prompts específicos de coding e scripts de release do produto upstream.
- Extensões arbitrárias, leitura de configurações do usuário do Pi e descoberta automática de ferramentas.
- Imagem/visão como requisito implícito; a v1 do Vox envia snapshots textuais e capability explícita.
- Execução direta de shell/arquivo/UI a partir do código importado; qualquer efeito precisa atravessar o broker Rust.

## Acoplamento e custo

- O agent core tem dependências de `pi-ai`, telemetry, `typebox`, `yaml`, `diff` e `ignore`; não é uma cópia pequena e isolada.
- O package de AI possui providers comerciais e SDKs de nuvem, portanto precisa de seleção por capability, redaction e uma política que não carregue credenciais por padrão.
- A extração deve carregar avisos MIT e inventário de dependências; não copiar o monorepo inteiro para o binário.
- O custo técnico inicial é controlado mantendo o clone apenas como fonte fixada e importando módulos mínimos para `packages/agent-core`; o custo de API permanece não autorizado até A05.

## Estratégia adotada

1. Manter este SHA como referência imutável de auditoria.
2. Criar um core Vox próprio supervisionado, com entrada/saída NDJSON e sem o executável Pi.
3. Extrair apenas contratos/loop/provider necessários, preservando notices e um mapa origem → destino.
4. Adaptar toda ferramenta para o broker e para autorização por chamada; remover bypass herdado.
5. Atualizações futuras exigem novo SHA, diff, revisão de segurança, testes contratuais e decisão explícita.

Esta tarefa fecha a proveniência e a estratégia de fork. A implementação da extração e os testes de “core sem Pi instalado” pertencem a T012–T016.
