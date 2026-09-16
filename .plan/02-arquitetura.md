# Arquitetura proposta

## Fronteiras

```text
Usuário ↔ Vox Desktop (Rust + egui/eframe)
                  ↕ eventos/comandos internos
         Supervisor + política + broker Rust
             ↕ IPC privado        ↕ workers nativos
         Vox Agent Core TS       xa11y / arquivos / shell / SO / áudio
             ↕
       adapters de providers LLM
```

Proposta A01: executar o **core próprio** TypeScript em processo privado supervisionado com runtime JS empacotado. Ele contém código selecionado do fork, não o executável, modo RPC ou CLI do Pi. Essa fronteira permite preservar o ecossistema TS e recuperar falhas sem congelar a interface. Comparar runtime empacotado e embedding no spike T002; decidir por evidência de tamanho, compatibilidade, cancelamento e distribuição. Se a preferência futura proibir qualquer processo auxiliar, reabrir A01 antes de implementar; não interpretar a proibição do CLI Pi como escolha automática de runtime.

O broker Rust é a autoridade de efeitos no computador. Ferramentas importadas do Pi devem chamar esse broker; caminhos antigos que executem shell/arquivos diretamente precisam ser removidos ou adaptados. Separação de processos sozinha não é sandbox: código TS comprometido ainda teria privilégios do usuário, salvo isolamento adicional. Não carregar extensões arbitrárias na v1.

GUI nunca aguarda operação bloqueante na thread de renderização. Workers usam filas limitadas, deadlines e propagação de cancelamento. Acesso a APIs nativas respeita seus requisitos de thread. Bibliotecas que possam bloquear indefinidamente devem ser isoladas em worker reiniciável se os spikes comprovarem necessidade.

## Componentes e responsabilidades

| Componente | Responsabilidade |
|---|---|
| desktop-ui | Janela, conversa, configurações essenciais, aprovação e estado de execução |
| supervisor | Vida dos processos, handshake, encerramento e recuperação |
| protocol | Schemas versionados compartilhados e fixtures de compatibilidade |
| policy/broker | Validar intenção autorizada, argumentos, alvos e executar ferramentas |
| desktop-access | Adaptar xa11y, normalizar árvores, resolver referências e verificar ações |
| native-platform | Apps, foco, atalhos, tray quando disponível, clipboard e capacidades |
| tool-runtime | Arquivos, shell, processos, limites, saída truncada e cancelamento |
| agent-core | Conversa, seleção de ferramentas, loop, contexto e eventos de execução |
| provider-adapters | Modelos, autenticação, streaming, limites e erros normalizados |
| session-store | Histórico, migrações, eventos e recuperação de execuções interrompidas |
| voice | Captura push-to-talk, transcrição, dispositivos e interrupção |

## Estrutura prevista; ainda não criada

```text
apps/desktop/
crates/{protocol,supervisor,policy,desktop-access,native-platform,tool-runtime,session-store,voice}/
packages/{agent-core,provider-adapters,protocol}/
upstream/pi/                 # origem preservada e manifesto de extração
schemas/
tests/{contracts,integration,e2e,fixtures,security}/
packaging/{linux,windows,macos}/
docs/{architecture,development,user,compatibility,licenses}/
```

## Estratégia do fork

1. Resolver upstream oficial a partir de pi.dev; fixar SHA, versão e licenças.
2. Criar fork com proveniência preservada; catalogar componentes e dependências transitivas antes da extração.
3. Candidatos: adapters de modelos, loop, streaming, mensagens, compactação e sessões. Shell/arquivos fornecem base de comportamento, mas efeitos passam pelo broker.
4. Excluir CLI, TUI, comandos específicos do produto, prompts voltados apenas a coding e descoberta automática de extensões/configurações do Pi.
5. Criar módulos próprios, preservar avisos/licenças e manter relatório origem → destino. Não presumir que todos os componentes são desacoplados.
6. Atualizações upstream passam por diff, revisão de segurança, testes contratuais e registro de decisão; não atualizar automaticamente produção.

## Modelo e contexto

Capability negotiation por provider/modelo: streaming, tools nativas, JSON estruturado, contexto e limites. Para modelo textual sem tools nativas, adapter de protocolo JSON validado com no máximo duas tentativas de reparo; texto livre nunca vira comando. Modelos incapazes de emitir chamadas válidas devem ser declarados incompatíveis, sem alegar que todo modelo terá igual competência.

Troca de provider somente em fronteira segura de turno, com conversão de mensagens, normalização de tool results e tratamento do orçamento. Snapshot do desktop sob demanda, limitado ao app/janela relevante, paginação e orçamento de tokens. Conteúdo externo é dado não confiável. Compactação preserva objetivo, restrições, aprovações com escopo, ações efetivadas e pendências; não renova permissões.

Uma execução com efeitos por desktop na v1. Outras conversas podem esperar em fila. Nenhuma repetição automática de ação de efeito incerto após falha de rede/processo.

## Decomposição adicional da aplicação nativa

A estrutura de módulos de UI, ViewModel/UiCommand, pipeline semântico, esquema de persistência e matriz de efeitos está detalhada em [implementação e entregáveis](12-detalhamento-tecnico-e-entregaveis.md). O estado de visibilidade da janela é separado do estado do run. Somente broker/supervisor determinam resultados e autorização; a UI renderiza eventos e envia intenções. Repaint é acionado por eventos, com limite por frame e prioridade de Parar. A janela pode mudar de modo sem reiniciar o core ou a conversa.
