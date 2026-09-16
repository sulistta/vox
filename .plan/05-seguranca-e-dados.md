# Segurança, privacidade e persistência

## Limites de confiança

Entrada direta do usuário define intenção; conteúdo de arquivos, páginas, acessibilidade, clipboard e saída de ferramentas é dado não confiável. Esses conteúdos não podem conceder permissões, alterar regras nem solicitar envio de dados por conta própria. Um app pode apresentar texto “ignore as regras”; a camada de política permanece fora do LLM.

Execução ocorre como usuário normal. Operações administrativas não têm elevação automática. O Vox tem poder de atuar no desktop; uma allowlist textual de comandos não fornece sandbox. Se permitir shell livre, mostrar claramente o escopo e aplicar limites reais de processo/ambiente onde disponíveis.

## Política proposta

| Classe | Exemplo | Comportamento padrão |
|---|---|---|
| Observação no alvo da tarefa | Ler janela ou diretório autorizado | Executar no escopo; coletar mínimo |
| Reversível e solicitada | Criar pasta, abrir app, escrever arquivo novo | Executar; verificar e registrar |
| Destrutiva ou sensível | Sobrescrever, encerrar processo alheio, alterar segurança | Preview concreto e aprovação contextual |
| Externa/irreversível | Enviar mensagem, publicar, comprar, apagar permanentemente | Aprovação explícita vinculada à ação |
| Fora do escopo/proibida | Ler segredo para enviar a terceiros ou burlar permissão | Bloquear e explicar |

Autorização prévia válida evita perguntas repetidas. Aprovações expiram quando mudam alvo, argumentos, run ou consequências; cancelamento revoga pendências. Mesma política para GUI, shell e arquivos, evitando contorno entre ferramentas. A classificação GUI pode ser incerta; ação ambígua com possível efeito externo pede esclarecimento.

Caminhos: canonicalização, symlinks, traversal, limites de diretório e revalidação no momento do acesso; evitar TOCTOU com primitives adequadas. Escritas atômicas quando aplicável, colisões explícitas, backup/reversão limitada e trash em vez de remoção permanente. Acesso a áreas sensíveis bloqueado por padrão com escopo configurável.

## Dados

Proposta A04: SQLite local para sessões, mensagens, runs, tool calls, aprovações e migrations; um proprietário de escrita. Esquemas versionados, timestamps UTC, timezone da sessão, transações e recuperação após encerramento abrupto. Conteúdo de ações em andamento é registrado antes do efeito; resultado registra evidência e incerteza.

Credenciais em keyring do SO, nunca em logs, configuração comum ou histórico. Em Linux sem keyring utilizável, pedir credencial temporária ou configurar armazenamento protegido explícito; não cair silenciosamente para texto puro. Core recebe somente a credencial necessária ao provider ativo e não a repassa ao modelo.

Histórico não é criptografia de disco: documentar proteção efetiva e exigir permissões locais restritas. Opções de retenção, apagar sessão/todos os dados e exportar de forma redigida. Logs rotacionados com redaction; nenhum upload automático. Telemetria desabilitada por padrão, opt-in para diagnóstico sem conteúdo de conversas. Áudio descartado após transcrição por padrão; screenshots não são capturados por padrão.

Antes do primeiro provider remoto, explicar que mensagens e observações selecionadas podem sair do computador. Aplicar minimização, filtros de segredos e preview quando relevante; filtros não garantem detectar todo segredo. Endpoints customizados exigem HTTPS, salvo localhost deliberado; manter verificação TLS e nunca incluir credencial na URL.

## Cenários obrigatórios

Injeção em árvore de acessibilidade/arquivo; chamada de tool adulterada; replay de aprovação; output com token; substituição de symlink; caminho com espaços/metacaracteres; cancelamento enquanto espera aprovação; restart após escrita; processo filho órfão; troca de janela durante aprovação; provider comprometido tentando ação fora da tarefa. Evidências em T050 e T054.
