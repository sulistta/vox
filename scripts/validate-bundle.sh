#!/usr/bin/env bash
set -euo pipefail

bundle="${1:-dist/vox-dev}"
case "$bundle" in
  ""|"/"|"."|"..")
    echo "refusing unsafe bundle path: $bundle" >&2
    exit 2
    ;;
esac
if [ ! -x "$bundle/run.sh" ]; then
  echo "bundle does not contain an executable run.sh: $bundle" >&2
  exit 3
fi

node scripts/verify-bundle-manifest.mjs "$bundle"
test -s "$bundle/sbom.cdx.json"
node -e 'const fs = require("node:fs"); const bom = JSON.parse(fs.readFileSync(process.argv[1], "utf8")); if (bom.bomFormat !== "CycloneDX" || bom.specVersion !== "1.5" || !Array.isArray(bom.components) || !Array.isArray(bom.dependencies)) process.exit(1);' "$bundle/sbom.cdx.json"
root="$(mktemp -d /tmp/vox-install-smoke.XXXXXX)"
trap 'rm -rf -- "$root"' EXIT
tampered="$root/tampered"
cp -a -- "$bundle" "$tampered"
printf '\ncorrupted smoke fixture\n' >>"$tampered/README.txt"
if node scripts/verify-bundle-manifest.mjs "$tampered" >"$root/tampered.log" 2>&1; then
  cat "$root/tampered.log" >&2
  echo "tampered bundle was accepted" >&2
  exit 4
fi
mkdir -p -- "$root/releases"
cp -a -- "$bundle" "$root/releases/v1"
ln -s -- "$root/releases/v1" "$root/current"
data="$root/data"

run_release() {
  local release="$1"
  local status=0
  set +e
  VOX_DATA_DIR="$data" timeout 6s "$release/run.sh" >"$root/run.log" 2>&1
  status=$?
  set -e
  if [ "$status" -ne 0 ] && [ "$status" -ne 124 ]; then
    cat "$root/run.log" >&2
    return "$status"
  fi
  test -f "$data/sessions.sqlite"
}

run_release "$root/current"
cp -a -- "$bundle" "$root/releases/v2"
ln -sfn -- "$root/releases/v2" "$root/current"
run_release "$root/current"
ln -sfn -- "$root/releases/v1" "$root/current"
run_release "$root/current"

install_prefix="$root/installed"
"$bundle/linux/install.sh" --prefix "$install_prefix"
test -x "$install_prefix/bin/vox"
status=0
set +e
VOX_DATA_DIR="$data" timeout 6s "$install_prefix/bin/vox" >"$root/installed.log" 2>&1
status=$?
set -e
if [ "$status" -ne 0 ] && [ "$status" -ne 124 ]; then
  cat "$root/installed.log" >&2
  exit "$status"
fi
test -f "$data/sessions.sqlite"

"$bundle/linux/uninstall.sh" --prefix "$install_prefix" --data-dir "$data"
test ! -e "$install_prefix"
test -f "$data/sessions.sqlite"
"$bundle/linux/install.sh" --prefix "$install_prefix"
"$bundle/linux/uninstall.sh" --prefix "$install_prefix" --data-dir "$data" --remove-data
test ! -e "$install_prefix"
rm -rf -- "$data"
test ! -e "$data"
rm -f -- "$root/current"
rm -rf -- "$root/releases"
echo "bundle install/update/rollback/uninstall smoke passed"
