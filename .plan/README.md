# Plano de desenvolvimento — Vox

Data: 16/09/2026. Nome de trabalho: Vox, inferido do diretório; marca final pendente.

## Estado atual

Repositório vazio na inspeção inicial, sem aplicação, testes, manifestos ou instruções locais. Esta entrega cria somente documentação. Nenhuma capacidade técnica foi implementada ou validada em execução. O contexto original do usuário é a autoridade de produto; propostas abaixo não substituem seus requisitos.

## Guia de leitura

1. [Contexto original](00-contexto-original.txt) — cópia integral da solicitação.
2. [Produto e escopo](01-produto.md) — requisitos, jornadas e limites das versões.
3. [Arquitetura](02-arquitetura.md) — componentes, processos, dados e reaproveitamento do Pi.
4. [Contratos](03-contratos.md) — IPC, ferramentas, percepção e execução.
5. [Plataformas e experiência](04-plataformas-e-ux.md) — compatibilidade, janela e voz.
6. [Segurança e dados](05-seguranca-e-dados.md) — autorização, privacidade e recuperação.
7. [Roadmap](06-roadmap.md) — fases, gates, esforço e caminho crítico.
8. [Tarefas](07-tarefas.md) — backlog executável com dependências e aceite.
9. [Validação e entrega](08-validacao-e-release.md) — testes, métricas e conclusão.
10. [Decisões, riscos e fontes](09-decisoes-riscos-fontes.md) — hipóteses e referências.
11. [Interface: janela flutuante](10-interface-janela-flutuante.md) — geometria, layout, tokens, modos e critérios visuais.
12. [Fluxos e estados da interface](11-fluxos-e-estados-da-interface.md) — interações, microcopy, teclado, voz e recuperação.
13. [Detalhamento técnico](12-detalhamento-tecnico-e-entregaveis.md) — módulos, contratos UI, dados, fixtures e entregáveis.

## Revisão de detalhamento

O plano contém 82 tarefas principais, todas pendentes. T065–T082 detalham interface e integração técnica e estão vinculadas aos gates; não formam uma fase posterior à v1. A interface obrigatória é uma **janela flutuante nativa, minimalista e moderna**, com modos de invocação, conversa e acompanhamento compacto. Leia os documentos 10 e 11 antes de implementar T037–T040. Não há aplicação ou protótipo implementado nesta revisão.

## Como executar

Começar por T001–T008. Resolver viabilidade antes de investir no produto inteiro. A primeira fatia funcional deve conectar conversa textual, modelo textual, snapshot semântico, ação verificada e cancelamento. Depois ampliar ferramentas, experiência, voz e distribuição.

Cada tarefa começa pendente (`[ ]`). Ao iniciar, registrar responsável e data; ao bloquear, registrar causa e próxima ação; ao concluir, usar `[x]` e anexar evidências de aceite. Dependências significam tarefas concluídas, salvo trabalho experimental claramente isolado. Não marcar sucesso apenas por compilar.

As tarefas usam prioridade P0 (fundação/requisito central), P1 (necessária à v1) e P2 (evolução). Estimativas P/M/G representam aproximadamente 1–2, 3–5 e 6–10 dias úteis de uma pessoa familiarizada com a stack; tarefas G devem ser subdivididas na execução. Estimativas não são compromissos.

Mudanças de escopo exigem atualizar requisitos, tarefas, matriz de validação e decisão relacionada. A v1 só termina após o gate G6, incluindo Linux/Wayland, Windows e macOS no escopo suportado e documentado.
