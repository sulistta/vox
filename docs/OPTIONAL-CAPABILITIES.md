# Capacidades opcionais pós-v1

## OCR/visão

Só considerar quando a árvore semântica não for suficiente. A saída deve ser texto observado com origem, confiança e limites; imagem não autoriza ação sozinha. Requer consentimento, redaction e benchmark separado.

## TTS/wake word

TTS pode ser avaliado como resposta explícita. Wake word exige captura contínua e portanto fica desabilitado por padrão, com decisão de privacidade separada. Não é dependência do fluxo textual.

## Extensibilidade

Plugins/MCP e automações agendadas só entram após modelo de confiança, escopo por ferramenta, revogação, sandbox e auditoria. A v1 não carrega plugin arbitrário.

## Memória/sincronização

Preferências e histórico local continuam separados de memória inferida. Sync futuro deve definir criptografia, exclusão, conflitos, exportação e retenção antes de coletar dados novos.
