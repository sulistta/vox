# Release e instalação

O checkout possui um bundle de desenvolvimento em
[`scripts/package-dev.sh`](../scripts/package-dev.sh). Ele é deliberadamente
rotulado como desenvolvimento. O bundle agora carrega o executável Node usado
pelo core e pode ser iniciado pelo run.sh sem Node instalado separadamente;
ele também contém `manifest.json` com SHA-256 de cada arquivo e uma cópia do
inventário de dependências, além de `sbom.cdx.json` em CycloneDX 1.5 com o
recorte de componentes Rust/Node usado no bundle dev. Isso ainda não é o
instalador de T055 nem fecha T060.

O recorte Linux também inclui `linux/install.sh` e `linux/uninstall.sh`: a
instalação é feita em um prefixo de usuário com troca atômica, o manifesto e o
SBOM são verificados antes da cópia e a desinstalação preserva os dados até
`--remove-data` ser informado explicitamente. O tarball Linux pode ser gerado
com `pnpm package:linux` e recebe um `.sha256` separado.

## Gate de artefato

Antes de publicar qualquer versão, o artefato deve conter a UI nativa, o core
privado e o runtime necessário, sem exigir Node/Pi/configuração do Pi no
computador do usuário. O checklist é:

1. gerar a partir de `Cargo.lock`, `pnpm-lock.yaml` e `rust-toolchain.toml`;
2. registrar plataforma, arquitetura, commit, tamanho e SHA-256;
3. gerar SBOM/notices e conferir licenças de Pi, xa11y, egui/eframe e SQLite;
4. executar instalação limpa, atualização, migração, rollback compatível e
   desinstalação com escolha explícita sobre os dados;
5. testar permissões, keyring, fontes, atalhos, geometria, residência, Parar,
   aprovação e recuperação fora da árvore de desenvolvimento;
6. publicar changelog, limitações e canal de suporte junto com os hashes.

O manifesto do bundle dev é uma evidência local de integridade do artefato, não
uma assinatura criptográfica de release. O SBOM dev é um inventário reproduzível
dos lockfiles/metadata disponíveis nesta árvore, não substitui notices,
licenças revisadas ou a validação de dependências transitivas do instalador.
O instalador final ainda deve gerar assinatura, SBOM/notices completos e
validação fora da árvore de desenvolvimento.

No Linux, o recorte reproduzível de instalação do bundle pode ser executado
com:

```sh
pnpm package:dev /tmp/vox-dev
pnpm bundle:validate /tmp/vox-dev
pnpm package:linux /tmp/vox-linux-dev.tar.gz
```

Esse smoke valida o manifesto e o SBOM, instala duas cópias temporárias pelo
launcher do bundle, preserva o SQLite durante uma troca v1→v2 e rollback v2→v1,
testa que a desinstalação mantém os dados até a
remoção explicitamente solicitada, rejeita uma cópia adulterada antes da
instalação e depois apaga apenas o diretório temporário.
Ele não substitui o instalador assinado nem a validação Windows/macOS.

## Estado atual

| Plataforma | Build/CI | Instalador | Sessão desktop real | Estado |
|---|---|---|---|---|
| Ubuntu 24.04 GNOME/Wayland x86_64 | local | bundle dev com Node embutido | processo nativo e probe xa11y; janela Vox não comprovada no AT-SPI | prévia técnica |
| Linux X11/KDE | CI/build | pendente | pendente | não publicar |
| Windows | CI/build | pendente | pendente | não publicar |
| macOS | CI/build | pendente | pendente | não publicar |

Não há assinatura, notarização, SBOM final ou promessa de compatibilidade
multiplataforma nesta execução.
