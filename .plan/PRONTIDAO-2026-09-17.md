# Auditoria de prontidão — 17/09/2026

## Veredito

O Vox funciona como um **preview de desenvolvimento integrado para Linux**. Ele
não está pronto para ser distribuído como o assistente de desktop universal
descrito no plano, nem para receber a marca de beta ou v1.

O motivo não é uma falha isolada de compilação. A implementação local passou
nos testes abaixo, mas o próprio plano exige evidência em modelos reais,
hardware, sistemas operacionais, instaladores e uso diário que esta máquina
não pode fabricar.

## Evidência reproduzida neste checkout

| Área | Resultado |
|---|---|
| TypeScript | `pnpm typecheck` e `pnpm test` passaram: 42 testes de protocolo, provider e core. |
| Rust | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` e `cargo test --workspace` passaram: 104 testes unitários/integração; os dois smokes AT-SPI opt-in foram executados separadamente. |
| Janela nativa | O smoke AT-SPI da própria janela passou em GNOME/Wayland: menu, compacto/expandido, Preferências e Sistema/Claro/Escuro são identificáveis pelo leitor de tela. |
| Acessibilidade de apps | A fixture GTK adversarial passou em AT-SPI real: PID direcionado, duplicidade, referências stale, diálogo, limites e bloqueio/redação de campos sensíveis. |
| Distribuição Linux de desenvolvimento | `pnpm package:dev` e `xvfb-run -a pnpm bundle:validate` passaram: manifesto, SBOM, instalação, atualização, rollback e remoção de dados. |
| Interface e segurança | A janela é nativa/egui, tem modos de invocação, conversa e compacto; Recolher mantém a única janela visível para preservar Expandir e Parar até existir rota de reativação; atividade enviada ao modelo aparece somente sob “Ver atividade” e com redação; aprovações são hash-bound, de uso único e agora usam rótulos concretos por efeito. |
| Provider remoto controlado | Um smoke OpenAI-compatible remoto concluiu conversa (2,322 s) e cancelamento (76 ms), com atividade redigida e zero tools liberadas. O resultado não mede qualidade, custo ou ferramentas reais. |
| Rascunhos | SQLite v4 salva rascunho redigido por sessão, restaura a última sessão válida e o exclui da exportação; teste consulta o banco e backup diretamente. |
| Estabilidade do core | Turno remoto lento preserva o core quando responde heartbeats; endpoint inválido retorna `CONFIG_ERROR` estruturado, sem encerrar o processo; SSE sem delimitador é limitado. |

O provider padrão continua sendo uma demonstração textual sem ferramentas. Uma
ação sobre o computador só pode acontecer depois de configurar explicitamente
um provider compatível, receber uma decisão estruturada válida e, quando
necessário, aprová-la na janela.

## Bloqueadores de release

| Gate do plano | O que ainda falta | Por que bloqueia |
|---|---|---|
| T051/T052 | Avaliação de qualidade/custo/latência, comportamento sob rate limit/falha e jornadas completas em aplicativos reais. | Um smoke remoto textual prova somente sessão, terminal, cancelamento e redação; não prova a capacidade de um modelo na automação de desktop. |
| T016/T050/T054 | Resolver TOCTOU de arquivos com operações ancoradas na raiz e testar troca concorrente de symlink/diretório; limitar observações não confiáveis do desktop/arquivo/clipboard por proveniência. | A validação canônica atual pode ser invalidada antes de escrita ou movimentação, e uma instrução injetada em observação ainda depende do prompt para ser ignorada. |
| T043–T045/T073 | STT integrado, dispositivo real, permissões, atalho global e jornada de voz completa. | A UI informa corretamente que voz não está pronta; não há experiência de voz utilizável. |
| T040/T069 | Tray, atalho global ou outra rota de reativação validada antes de esconder/minimizar a única janela. | Sem uma rota de volta, ocultar o Vox pode deixar a conversa e o caminho de Parar inacessíveis; a UI atual só recolhe a janela visível. |
| T006/T007/T048/T049 | Testes reais em Windows, macOS, X11/KDE e ambientes Wayland publicados. | A promessa é universal e as APIs de acessibilidade/permissões variam por plataforma. |
| T074/T075 | QA real de teclado, IME, foco, scroll, leitor de tela, 100/150/200% e larguras 320/360/440/640; screenshots de aprovação, erro e voz. | Um smoke semântico e uma captura Xvfb não provam a experiência final. |
| T055–T060/T082 | Instaladores finais assinados, matriz limpa, atualização entre versões reais, documentação de release e beta de uso diário. | O artefato atual é um bundle Linux de desenvolvimento, não um canal de distribuição. |

## Decisão operacional

Pode ser usado para desenvolvimento e demonstração controlada no Linux desta
máquina. Não deve ser anunciado, instalado para usuários finais ou tratado
como produto concluído antes de fechar os gates acima. As pendências são
mantidas no plano em vez de serem convertidas em sucesso por testes locais.
