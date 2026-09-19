# Execução — Jev como agente principal

## Implementado

- Jev é obrigatório e escolhe o próximo passo por perguntas `Choice`, `Score` e `Noul`.
- O provider OpenAI-compatible é obrigatório e só produz `TaskSpec`, candidatos e texto final. Ele não possui contrato de ferramenta.
- Cada envio cria um `TaskGrant` por `run_id`; o grant é registrado no SQLite, transmitido pelo IPC e validado pelo broker antes de uma execução. Ele limita ferramentas ao pedido, exige processos/PIDs observados, executáveis resolvidos, snapshots atuais e raízes configuradas.
- A janela não mostra mais modal de aprovação por comando. O usuário interrompe a tarefa pelo botão **Parar**.
- O broker mantém referências de snapshot com TTL, impede reaplicação automática por meio do journal e preserva efeitos incertos como `unknown`.
- Preferências antigas são migradas uma vez para `text_*` e `agent_*`. Segredos passam de `vox` para `vox-text` e de `vox-jev` para `vox-agent` somente após a gravação no destino confirmar.
- A interface apresenta **Texto e planejamento** e **Agente Jev**, com modelos, contas do keyring, chaves mascaradas e teste de conexão para os dois serviços.

## Limitações verificadas

- A validação de elemento de desktop depende da árvore de acessibilidade que o aplicativo publicar. Para Discord/Electron sem essa árvore, o Vox pede que o usuário o reinicie com acessibilidade do renderer.
- O pacote não afirma sucesso quando a ferramenta retorna efeito `unknown` ou sem evidência de verificação.
- Os testes locais do adapter validam o protocolo e falhas de transporte; acesso real à TypeSafe continua dependendo de uma chave válida.

## Validação executada

- `pnpm --filter @vox/protocol test`
- `pnpm --filter @vox/provider-adapters test`
- `pnpm --filter @vox/agent-core test`
- `cargo test -p vox-policy -p vox-broker -p vox-session-store`
- `cargo test -p vox-desktop -p vox-supervisor --no-run`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `pnpm package:dev` e `pnpm bundle:validate dist/vox-dev` — pacote dev, SBOM, manifesto e smoke de instalar/trocar/desinstalar verificados.

### Correção da geração de candidatos

As duas tentativas com Discord e Spotify terminaram em falha. O histórico não preserva a resposta rejeitada, portanto o campo específico não foi identificado retrospectivamente. Corrigido o exemplo inválido de JSON no prompt de candidatos; documentados campos, limites, IDs e ausência de `effect`. O validador agora informa o campo/regra violado sem incluir valores recebidos e rejeita IDs reservados ao controle do Jev. Uma resposta inválida permite somente uma nova geração textual, antes de qualquer despacho de ferramenta. Testes locais cobrem recuperação, falha persistente, limite de duas chamadas e ausência do conteúdo rejeitado no diagnóstico. Typecheck e testes de adapters/core passaram. Isso não comprova conclusão de tarefas reais no Discord/Spotify.
