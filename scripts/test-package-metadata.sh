#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <recording/package-audit-directory>" >&2
  exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH='' cd -- "$script_dir/.." && pwd)
python_bin=${PYTHON:-python3}
audit_dir=$1

mkdir -p "$audit_dir"
audit_dir=$(realpath "$audit_dir")
case "$audit_dir/" in
  "$repo_root/"*)
    echo "error: package audit output must be outside the Git repository: $audit_dir" >&2
    exit 2
    ;;
esac

version=$(
  "$python_bin" -c \
    'import pathlib, sys, tomllib; print(tomllib.loads(pathlib.Path(sys.argv[1]).read_text())["package"]["version"])' \
    "$repo_root/Cargo.toml"
)
tag="v$version"

for target_arch in x86_64 aarch64; do
  target_dir="$audit_dir/$target_arch"
  artifact="$target_dir/capglyph-linux-$target_arch"
  mkdir -p "$target_dir"
  install -m 0755 /bin/true "$artifact"
  "$script_dir/package-linux.sh" "$tag" "$target_arch" "$artifact" "$target_dir/packages"
done

if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag v999.0.0 \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml" \
  >"$audit_dir/cargo-tag-mismatch.log" 2>&1; then
  echo "error: Cargo/tag mismatch was accepted" >&2
  exit 1
fi

if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag 0.1.7 \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml" \
  >"$audit_dir/malformed-tag.log" 2>&1; then
  echo "error: malformed release tag was accepted" >&2
  exit 1
fi

malformed_semver_log="$audit_dir/malformed-semver.log"
if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag v1.2.3-alpha..1 \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml" \
  >"$malformed_semver_log" 2>&1; then
  echo "error: malformed SemVer release tag was accepted" >&2
  exit 1
fi
if ! grep -q "complete v-prefixed SemVer" "$malformed_semver_log"; then
  echo "error: malformed SemVer did not fail at tag validation" >&2
  exit 1
fi

manifest_fixture="$audit_dir/manifest-mismatch"
mkdir -p "$manifest_fixture/crates/capglyph-server"
install -m 0644 "$repo_root/Cargo.toml" "$manifest_fixture/Cargo.toml"
"$python_bin" -c \
  'import pathlib, sys; source=pathlib.Path(sys.argv[1]).read_text(); needle=f"version = \"{sys.argv[3]}\""; assert needle in source; pathlib.Path(sys.argv[2]).write_text(source.replace(needle, "version = \"0.0.0\"", 1))' \
  "$repo_root/crates/capglyph-server/Cargo.toml" \
  "$manifest_fixture/crates/capglyph-server/Cargo.toml" \
  "$version"
if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag "$tag" \
  --cargo-manifest "$manifest_fixture/Cargo.toml" \
  --nfpm-config "$repo_root/nfpm.yaml" \
  >"$audit_dir/server-manifest-mismatch.log" 2>&1; then
  echo "error: server Cargo/tag mismatch was accepted" >&2
  exit 1
fi

bad_config="$audit_dir/nfpm-static-version.yaml"
"$python_bin" -c \
  'import pathlib, sys; source=pathlib.Path(sys.argv[1]).read_text(); pathlib.Path(sys.argv[2]).write_text(source.replace("__CAPGLYPH_VERSION__", "0.0.0"))' \
  "$repo_root/nfpm.yaml" "$bad_config"
if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag "$tag" \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$bad_config" \
  >"$audit_dir/nfpm-mismatch.log" 2>&1; then
  echo "error: static nFPM version was accepted" >&2
  exit 1
fi

duplicate_config="$audit_dir/nfpm-duplicate-version.yaml"
"$python_bin" -c \
  'import pathlib, sys; source=pathlib.Path(sys.argv[1]).read_text(); needle="version: __CAPGLYPH_VERSION__"; assert source.count(needle) == 1; pathlib.Path(sys.argv[2]).write_text(source.replace(needle, needle + "\nversion: 0.0.0"))' \
  "$repo_root/nfpm.yaml" "$duplicate_config"
if "$python_bin" "$script_dir/verify-package-metadata.py" contract \
  --tag "$tag" \
  --cargo-manifest "$repo_root/Cargo.toml" \
  --nfpm-config "$duplicate_config" \
  >"$audit_dir/nfpm-duplicate-version.log" 2>&1; then
  echo "error: duplicate nFPM version field was accepted" >&2
  exit 1
fi

if "$python_bin" "$script_dir/verify-package-metadata.py" packages \
  --tag v999.0.0 \
  --target-arch x86_64 \
  --deb "$audit_dir/x86_64/packages/capglyph-linux-x86_64.deb" \
  --rpm "$audit_dir/x86_64/packages/capglyph-linux-x86_64.rpm" \
  --arch "$audit_dir/x86_64/packages/capglyph-linux-x86_64.pkg.tar.zst" \
  >"$audit_dir/package-version-mismatch.log" 2>&1; then
  echo "error: package version mismatch was accepted" >&2
  exit 1
fi

if "$python_bin" "$script_dir/verify-package-metadata.py" packages \
  --tag "$tag" \
  --target-arch aarch64 \
  --deb "$audit_dir/x86_64/packages/capglyph-linux-x86_64.deb" \
  --rpm "$audit_dir/x86_64/packages/capglyph-linux-x86_64.rpm" \
  --arch "$audit_dir/x86_64/packages/capglyph-linux-x86_64.pkg.tar.zst" \
  >"$audit_dir/package-architecture-mismatch.log" 2>&1; then
  echo "error: package architecture mismatch was accepted" >&2
  exit 1
fi

echo "package metadata integration checks passed for $tag (x86_64 and aarch64)"
