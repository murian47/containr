#!/usr/bin/env bash
set -euo pipefail

# Remove a containr installation created by packaging/macos/install.sh.
#
# Default layout:
# - binary payload: /usr/local/libexec/containr/containr
# - wrapper:        /usr/local/bin/containr
# - themes:         /usr/local/share/containr/themes
#
# Override with:
#   --prefix <dir>
#   --bin-dir <dir>
#   --libexec-dir <dir>
#   --share-dir <dir>
#   --keep-themes

usage() {
  cat <<'EOF'
Usage: packaging/macos/uninstall.sh [options]

Options:
  --prefix <dir>         Install prefix (default: /usr/local)
  --bin-dir <dir>        Wrapper dir (default: <prefix>/bin)
  --libexec-dir <dir>    Binary payload dir (default: <prefix>/libexec/containr)
  --share-dir <dir>      Shared data dir (default: <prefix>/share/containr)
  --keep-themes          Keep installed themes
  -h, --help             Show this help

Examples:
  packaging/macos/uninstall.sh
  packaging/macos/uninstall.sh --prefix "$HOME/.local"
  packaging/macos/uninstall.sh --keep-themes
EOF
}

prefix="/usr/local"
bin_dir=""
libexec_dir=""
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
    --libexec-dir)
      libexec_dir="${2:?missing value for --libexec-dir}"
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
if [[ -z "${libexec_dir}" ]]; then
  libexec_dir="${prefix}/libexec/containr"
fi
if [[ -z "${share_dir}" ]]; then
  share_dir="${prefix}/share/containr"
fi

wrapper_path="${bin_dir}/containr"
payload_path="${libexec_dir}/containr"
themes_path="${share_dir}/themes"

if [[ -f "${wrapper_path}" ]]; then
  echo "Removing wrapper ${wrapper_path}"
  rm -f "${wrapper_path}"
fi

if [[ -f "${payload_path}" ]]; then
  echo "Removing binary ${payload_path}"
  rm -f "${payload_path}"
fi

if [[ -d "${libexec_dir}" ]]; then
  rmdir "${libexec_dir}" 2>/dev/null || true
fi

if [[ "${keep_themes}" -eq 0 && -d "${themes_path}" ]]; then
  echo "Removing themes ${themes_path}"
  rm -rf "${themes_path}"
fi

if [[ -d "${share_dir}" ]]; then
  rmdir "${share_dir}" 2>/dev/null || true
fi

echo "containr uninstall completed."
