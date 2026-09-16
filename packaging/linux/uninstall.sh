#!/usr/bin/env bash
set -euo pipefail

prefix="${VOX_INSTALL_PREFIX:-${HOME:?}/.local/vox}"
remove_data=0
data_dir="${VOX_DATA_DIR:-${XDG_DATA_HOME:-${HOME:?}/.local/share}/vox}"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix)
      [ "$#" -ge 2 ] || { echo "--prefix exige um caminho" >&2; exit 2; }
      prefix="$2"
      shift 2
      ;;
    --remove-data)
      remove_data=1
      shift
      ;;
    --data-dir)
      [ "$#" -ge 2 ] || { echo "--data-dir exige um caminho" >&2; exit 2; }
      data_dir="$2"
      shift 2
      ;;
    -h|--help)
      echo "uso: $0 --prefix DIR [--remove-data] [--data-dir DIR]"
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
    echo "refusing unsafe uninstall prefix: $prefix" >&2
    exit 2
    ;;
esac

if [ -e "$prefix" ] || [ -L "$prefix" ]; then
  tombstone="${prefix}.removed.$$"
  mv -- "$prefix" "$tombstone"
  rm -rf -- "$tombstone"
fi

if [ "$remove_data" -eq 1 ]; then
  case "$data_dir" in
    ""|"/"|"$HOME"|"$HOME/"|"$HOME/.local"|"$HOME/.local/"|"$HOME/.local/share"|"$HOME/.local/share/")
      echo "refusing unsafe data directory: $data_dir" >&2
      exit 2
      ;;
  esac
  rm -rf -- "$data_dir"
  echo "Dados removidos: $data_dir"
else
  echo "Instalação removida; dados preservados em $data_dir"
fi
