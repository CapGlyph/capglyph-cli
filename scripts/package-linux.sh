#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "usage: $0 <v-tag> <x86_64|aarch64> <artifact> <output-directory>" >&2
  exit 2
}

if [[ $# -ne 4 ]]; then
  usage
fi

release_tag=$1
target_arch=$2
artifact=$3
output_dir=$4

script_dir=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
python_bin=${PYTHON:-python3}
nfpm_bin=${NFPM:-nfpm}

case "$target_arch" in
  x86_64 | aarch64)
    package_arch=$target_arch
    ;;
  *)
    echo "error: unsupported target architecture: $target_arch" >&2
    exit 2
    ;;
esac

if [[ ! -f "$artifact" ]]; then
  echo "error: package artifact does not exist: $artifact" >&2
  exit 1
fi

if ! command -v "$nfpm_bin" >/dev/null 2>&1; then
  echo "error: nFPM executable not found: $nfpm_bin" >&2
  exit 1
fi

"$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag "$release_tag" \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml"

mkdir -p "$output_dir"
artifact=$(realpath "$artifact")
output_dir=$(realpath "$output_dir")

deb="$output_dir/capglyph-linux-$package_arch.deb"
rpm="$output_dir/capglyph-linux-$package_arch.rpm"
arch="$output_dir/capglyph-linux-$package_arch.pkg.tar.zst"
rendered_config="$output_dir/nfpm-$package_arch.yaml"

"$python_bin" "$script_dir/verify-package-metadata.py" render \
  --tag "$release_tag" \
  --target-arch "$target_arch" \
  --source "$artifact" \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml" \
  --output "$rendered_config"

"$nfpm_bin" package --config "$rendered_config" --packager deb --target "$deb"
"$nfpm_bin" package --config "$rendered_config" --packager rpm --target "$rpm"
"$nfpm_bin" package --config "$rendered_config" --packager archlinux --target "$arch"

"$python_bin" "$script_dir/verify-package-metadata.py" packages \
  --tag "$release_tag" \
  --target-arch "$target_arch" \
  --deb "$deb" \
  --rpm "$rpm" \
  --arch "$arch"
