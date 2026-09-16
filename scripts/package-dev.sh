#!/usr/bin/env bash
set -euo pipefail

source "${HOME}/.cargo/env" 2>/dev/null || true

out_dir="${1:-dist/vox-dev}"
case "$out_dir" in
  ""|"/"|"."|"..")
    echo "refusing unsafe output directory: $out_dir" >&2
    exit 2
    ;;
esac
rm -rf -- "$out_dir"
mkdir -p -- "$out_dir/core" "$out_dir/bin" "$out_dir/runtime"
pnpm build
cargo build -p vox-desktop
cp -- target/debug/vox-desktop "$out_dir/bin/vox-desktop"
cp -R -- packages/agent-core/dist "$out_dir/core/agent-core"
mkdir -p -- "$out_dir/core/node_modules/@vox/protocol" "$out_dir/core/node_modules/@vox/provider-adapters"
cp -R -- packages/protocol/dist "$out_dir/core/node_modules/@vox/protocol/dist"
cp -- packages/protocol/package.json "$out_dir/core/node_modules/@vox/protocol/package.json"
cp -R -- packages/provider-adapters/dist "$out_dir/core/node_modules/@vox/provider-adapters/dist"
cp -- packages/provider-adapters/package.json "$out_dir/core/node_modules/@vox/provider-adapters/package.json"
node_bin="${VOX_NODE:-$(command -v node || true)}"
if [ -z "$node_bin" ] || [ ! -x "$node_bin" ]; then
  echo "a compatible Node.js executable is required to create the dev bundle" >&2
  exit 3
fi
cp -- "$node_bin" "$out_dir/runtime/node"
chmod +x "$out_dir/runtime/node"
cp -- docs/DEPENDENCIES.md "$out_dir/DEPENDENCIES.md"
cp -- scripts/verify-bundle-manifest.mjs "$out_dir/verify-bundle-manifest.mjs"
mkdir -p -- "$out_dir/linux"
cp -- packaging/linux/install.sh "$out_dir/linux/install.sh"
cp -- packaging/linux/uninstall.sh "$out_dir/linux/uninstall.sh"
cat >"$out_dir/run.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
bundle_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
export VOX_NODE="${VOX_NODE:-$bundle_dir/runtime/node}"
export VOX_AGENT_ENTRY="${VOX_AGENT_ENTRY:-$bundle_dir/core/agent-core/main.js}"
exec "$bundle_dir/bin/vox-desktop" "$@"
EOF
chmod +x "$out_dir/run.sh"
cat >"$out_dir/README.txt" <<'EOF'
Vox development bundle

This bundle contains the Node.js executable used during packaging and can be
started with ./run.sh. It is still a development bundle: it has not passed
the multi-platform installer, signing, SBOM, update, rollback or permission
gates from T055/T056/T082.

On Linux, linux/install.sh installs this bundle atomically under a user prefix;
linux/uninstall.sh preserves application data unless --remove-data is explicit.
EOF
node scripts/write-bundle-sbom.mjs "$out_dir"
node scripts/write-bundle-manifest.mjs "$out_dir"
echo "created $out_dir"
