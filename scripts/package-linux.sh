#!/usr/bin/env bash
set -euo pipefail

source "${HOME}/.cargo/env" 2>/dev/null || true

bundle="${VOX_LINUX_BUNDLE:-dist/vox-linux-dev}"
archive="${1:-dist/vox-linux-dev.tar.gz}"
case "$archive" in
  ""|"/"|"."|"..")
    echo "refusing unsafe archive path: $archive" >&2
    exit 2
    ;;
esac

./scripts/package-dev.sh "$bundle"
mkdir -p -- "$(dirname -- "$archive")"
rm -f -- "$archive" "$archive.sha256"
tar --sort=name --mtime='UTC 1970-01-01' --owner=0 --group=0 --numeric-owner \
  -czf "$archive" -C "$(dirname -- "$bundle")" "$(basename -- "$bundle")"
sha256sum "$archive" >"$archive.sha256"
echo "created $archive"
echo "created $archive.sha256"
