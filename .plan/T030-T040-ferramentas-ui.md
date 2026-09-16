# T030–T040 — ferramentas e experiência nativa

Data: 16/09/2026.

O runtime agora tem argv sem shell, cwd dentro das raízes permitidas, timeout/cancelamento, drenagem concorrente de stdout/stderr, truncamento, leitura UTF-8/binária, busca, listagem, escrita atômica, movimentação sem overwrite implícito, processos por PID+comando, lançamento/abertura com estado `unknown` quando só houve spawn, clipboard sob demanda com backend por SO e teste de symlink fora do escopo.

A janela egui possui compositor multilinha, histórico, streaming sem duplicação, rascunho preservado, estados de execução, Parar, modo compacto, faixa de atividade, aprovação pendente e menu nativo. O reducer de apresentação tem testes para não duplicar streaming e não perder run ao compactar.

A matriz consolidada de pré-condição, pós-condição, timeout, cancelamento e
repetição está em [TOOL-MATRIX](../docs/TOOL-MATRIX.md). A prova de aprovação
específica → uma escrita → replay bloqueado está no teste de broker; a UI
exibe a tool e os argumentos antes de autorizar.

O recorte local de clipboard agora tem leitura, escrita, limite de bytes, cancelamento bounded e aprovação no broker, cobertos por fixtures de backend. O aceite total ainda exige teste em sessões reais Wayland/X11/Windows/macOS, foco cross-compositor, MIME além de texto e confirmação de ações em apps reais, aprovação interativa visual e pacote instalável. A UI não simula essas provas.
