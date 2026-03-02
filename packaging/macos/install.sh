#!/usr/bin/env bash
set -euo pipefail

# Install containr on macOS from the current source tree.
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
#   --source-binary <file>
#   --skip-build

usage() {
  cat <<'EOF'
Usage: packaging/macos/install.sh [options]

Options:
  --prefix <dir>         Install prefix (default: /usr/local)
  --bin-dir <dir>        Wrapper install dir (default: <prefix>/bin)
  --libexec-dir <dir>    Binary payload dir (default: <prefix>/libexec/containr)
  --share-dir <dir>      Shared data dir (default: <prefix>/share/containr)
  --source-binary <file> Use an existing containr binary instead of target/release/containr
  --skip-build           Do not run cargo build --release
  -h, --help             Show this help

Examples:
  packaging/macos/install.sh
  packaging/macos/install.sh --prefix "$HOME/.local"
  packaging/macos/install.sh --skip-build --source-binary target/release/containr
EOF
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"

prefix="/usr/local"
bin_dir=""
libexec_dir=""
share_dir=""
source_binary=""
skip_build=0

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
    --source-binary)
      source_binary="${2:?missing value for --source-binary}"
      shift 2
      ;;
    --skip-build)
      skip_build=1
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

if [[ -z "${source_binary}" ]]; then
  source_binary="${repo_root}/target/release/containr"
fi

themes_src="${repo_root}/themes"

if [[ ! -d "${themes_src}" ]]; then
  echo "themes directory not found: ${themes_src}" >&2
  exit 1
fi

if [[ "${skip_build}" -eq 0 ]]; then
  echo "Building containr release binary..."
  (
    cd "${repo_root}"
    cargo build --release
  )
fi

if [[ ! -x "${source_binary}" ]]; then
  echo "containr binary not found or not executable: ${source_binary}" >&2
  exit 1
fi

if [[ ! -d "${prefix}" ]]; then
  mkdir -p "${prefix}"
fi

mkdir -p "${bin_dir}" "${libexec_dir}" "${share_dir}/themes"

echo "Installing binary payload to ${libexec_dir}/containr"
install -m 0755 "${source_binary}" "${libexec_dir}/containr"

echo "Installing themes to ${share_dir}/themes"
rm -rf "${share_dir}/themes"
mkdir -p "${share_dir}/themes"
cp -R "${themes_src}/." "${share_dir}/themes/"

wrapper_path="${bin_dir}/containr"
echo "Installing wrapper to ${wrapper_path}"
cat > "${wrapper_path}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
exec "${libexec_dir}/containr" "\$@"
EOF
chmod 0755 "${wrapper_path}"

cat <<EOF

containr installed successfully.

Binary:
  ${libexec_dir}/containr

Wrapper:
  ${wrapper_path}

Themes:
  ${share_dir}/themes

If "${bin_dir}" is not in PATH, add it before running containr.
EOF
