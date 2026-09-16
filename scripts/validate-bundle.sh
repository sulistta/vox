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

rm -f -- "$root/current"
rm -rf -- "$root/releases"
test -f "$data/sessions.sqlite"
rm -rf -- "$data"
test ! -e "$data"
echo "bundle install/update/rollback/uninstall smoke passed"
