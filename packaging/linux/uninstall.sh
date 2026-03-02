#!/usr/bin/env bash
set -euo pipefail

# Remove a containr installation created by packaging/linux/install.sh.
#
# Default layout:
# - binary: /usr/local/bin/containr
# - themes: /usr/local/share/containr/themes
#
# Override with:
#   --prefix <dir>
#   --bin-dir <dir>
#   --share-dir <dir>
#   --keep-themes

usage() {
  cat <<'EOF'
Usage: packaging/linux/uninstall.sh [options]

Options:
  --prefix <dir>         Install prefix (default: /usr/local)
  --bin-dir <dir>        Wrapper dir (default: <prefix>/bin)
  --bin-dir <dir>        Binary dir (default: <prefix>/bin)
  --share-dir <dir>      Shared data dir (default: <prefix>/share/containr)
  --keep-themes          Keep installed themes
  -h, --help             Show this help

Examples:
  packaging/linux/uninstall.sh
  packaging/linux/uninstall.sh --prefix "$HOME/.local"
  packaging/linux/uninstall.sh --keep-themes
EOF
}

prefix="/usr/local"
bin_dir=""
share_dir=""
keep_themes=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix)
      prefix="${2:?missing value for --prefix}"
      shift 2
      ;;
    --bin-dir)
      bin_dir="${2:?missing value for --bin-dir}"
      shift 2
      ;;
    --share-dir)
      share_dir="${2:?missing value for --share-dir}"
      shift 2
      ;;
    --keep-themes)
      keep_themes=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "${bin_dir}" ]]; then
  bin_dir="${prefix}/bin"
fi
if [[ -z "${share_dir}" ]]; then
  share_dir="${prefix}/share/containr"
fi

binary_path="${bin_dir}/containr"
themes_path="${share_dir}/themes"

if [[ -f "${binary_path}" ]]; then
  echo "Removing binary ${binary_path}"
  rm -f "${binary_path}"
fi

if [[ "${keep_themes}" -eq 0 && -d "${themes_path}" ]]; then
  echo "Removing themes ${themes_path}"
  rm -rf "${themes_path}"
fi

if [[ -d "${share_dir}" ]]; then
  rmdir "${share_dir}" 2>/dev/null || true
fi

echo "containr uninstall completed."
