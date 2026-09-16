#!/usr/bin/env bash
set -euo pipefail

bundle_dir="$(cd -- "$(dirname -- "$0")/.." && pwd)"
prefix="${VOX_INSTALL_PREFIX:-${HOME:?}/.local/vox}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || { echo "--prefix exige um caminho" >&2; exit 2; }
      prefix="$2"
      shift 2
      ;;
    --bundle)
      [ "$#" -ge 2 ] || { echo "--bundle exige um caminho" >&2; exit 2; }
      bundle_dir="$(cd -- "$2" && pwd)"
      shift 2
      ;;
    -h|--help)
      echo "uso: $0 [--bundle DIR] [--prefix DIR]"
      exit 0
      ;;
    *)
      echo "opção desconhecida: $1" >&2
      exit 2
      ;;
  esac
done

case "$prefix" in
  ""|"/"|"$HOME"|"$HOME/"|"$HOME/.local"|"$HOME/.local/")
    echo "refusing unsafe installation prefix: $prefix" >&2
    exit 2
    ;;
esac

test -x "$bundle_dir/run.sh"
test -x "$bundle_dir/runtime/node"
test -f "$bundle_dir/verify-bundle-manifest.mjs"
test -f "$bundle_dir/sbom.cdx.json"
"$bundle_dir/runtime/node" "$bundle_dir/verify-bundle-manifest.mjs" "$bundle_dir"

parent="$(dirname -- "$prefix")"
mkdir -p -- "$parent"
stage="$(mktemp -d "$parent/.vox-install.XXXXXX")"
backup="${prefix}.previous.$$"
cleanup() {
  rm -rf -- "$stage"
  rm -rf -- "$backup"
}
trap cleanup EXIT

mkdir -p -- "$stage/lib/vox" "$stage/bin" "$stage/share/applications"
cp -a -- "$bundle_dir/." "$stage/lib/vox/"
cat >"$stage/bin/vox" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
install_root="$(cd -- "$(dirname -- "$0")/.." && pwd)"
exec "$install_root/lib/vox/run.sh" "$@"
EOF
chmod +x "$stage/bin/vox"
cat >"$stage/share/applications/vox.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Vox
Comment=Assistente de desktop Vox
Exec=$prefix/bin/vox
Terminal=false
Categories=Utility;Accessibility;
StartupNotify=true
EOF

if [ -e "$prefix" ] || [ -L "$prefix" ]; then
  mv -- "$prefix" "$backup"
fi
mv -- "$stage" "$prefix"
trap - EXIT
rm -rf -- "$backup"
echo "Vox instalado em $prefix"
echo "Os dados do usuário permanecem no diretório configurado pelo Vox."
