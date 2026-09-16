# Manutenção, suporte e vulnerabilidades

## Triagem

1. confirmar versão/commit, SO, compositor, arquitetura e modo de instalação;
2. pedir reprodução mínima sem conversa privada, chave, áudio ou conteúdo do
   desktop;
3. separar crash, falha de provider, permissão de acessibilidade, política,
   efeito externo desconhecido e erro de UI;
4. classificar impacto e registrar se o resultado foi `success`, `unknown`,
   cancelado ou negado;
5. reproduzir em fixture sintética antes de pedir uma jornada em app real.

## Diagnóstico

O pacote de diagnóstico deve incluir apenas versão, capabilities, estados,
tempos, códigos de erro e limites; mensagens e argumentos devem ser redigidos.
Nunca anexar `.env`, tokens, exportação não revisada, áudio, clipboard ou
snapshot bruto de um app real. O export do SQLite já exclui segredos por
definição, mas ainda deve ser revisado antes do envio.

## Vulnerabilidades

Falhas de autorização, exfiltração, bypass do broker, replay de approval,
execução fora da raiz ou falsa confirmação de efeito são críticas. Não publicar
detalhes exploráveis antes de correção e revisão. O responsável deve:

- confirmar recebimento de forma privada;
- preservar uma reprodução mínima sem segredo;
- atribuir severidade, escopo e versão afetada;
- corrigir com teste regressivo e atualizar o changelog;
- invalidar tokens, artefatos e instruções de diagnóstico comprometidos;
- publicar a correção e a limitação residual somente após validação.

## Dependências e upstream

Revisões de Pi/xa11y e dependências devem registrar SHA/versão, licença,
motivação, impacto e resultado de `pnpm test`/`cargo test`. O fork seletivo de
Pi não deve ler configuração ou extensões do usuário.
