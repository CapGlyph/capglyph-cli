#!/usr/bin/env python3
"""Fail-closed validation for CapGlyph native package metadata."""

from __future__ import annotations

import argparse
import gzip
import io
import json
import lzma
import re
import struct
import subprocess
import sys
import tarfile
from pathlib import Path

import tomllib

CORE_VERSION = r"(?:0|[1-9][0-9]*)"
PRERELEASE_IDENTIFIER = r"(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
BUILD_IDENTIFIER = r"[0-9A-Za-z-]+"
TAG_RE = re.compile(
    rf"^v(?P<version>{CORE_VERSION}\.{CORE_VERSION}\.{CORE_VERSION}"
    rf"(?:-{PRERELEASE_IDENTIFIER}(?:\.{PRERELEASE_IDENTIFIER})*)?"
    rf"(?:\+{BUILD_IDENTIFIER}(?:\.{BUILD_IDENTIFIER})*)?)$"
)
EXPECTED_CONFIG_FIELDS = {
    "arch": "__CAPGLYPH_ARCH__",
    "version": "__CAPGLYPH_VERSION__",
    "release": "1",
}
ARCHITECTURES = {
    "x86_64": {"deb": "amd64", "rpm": "x86_64", "arch": "x86_64"},
    "aarch64": {"deb": "arm64", "rpm": "aarch64", "arch": "aarch64"},
}


class MetadataError(ValueError):
    """Raised when release metadata is absent, malformed, or inconsistent."""


def version_from_tag(tag: str) -> str:
    match = TAG_RE.fullmatch(tag)
    if not match:
        raise MetadataError(
            f"release tag must be a complete v-prefixed SemVer: {tag!r}"
        )
    return match.group("version")


def cargo_package_version(manifest: Path, expected_name: str) -> str:
    try:
        package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as error:
        raise MetadataError(
            f"cannot read Cargo package metadata from {manifest}: {error}"
        ) from error
    if package.get("name") != expected_name:
        raise MetadataError(
            f"Cargo package name mismatch in {manifest}: expected {expected_name!r}, "
            f"got {package.get('name')!r}"
        )
    version = package.get("version")
    if not isinstance(version, str) or not version:
        raise MetadataError(f"Cargo package version is missing in {manifest}")
    return version


def yaml_scalar(text: str, key: str) -> str:
    matches = list(
        re.finditer(
            rf"(?m)^{re.escape(key)}:\s*(?:\"([^\"]*)\"|'([^']*)'|([^#\s]+))"
            r"\s*(?:#.*)?$",
            text,
        )
    )
    if len(matches) != 1:
        raise MetadataError(
            f"nFPM config must contain exactly one unambiguous top-level {key!r} field; "
            f"found {len(matches)}"
        )
    match = matches[0]
    return next(value for value in match.groups() if value is not None)


def validate_contract(tag: str, cargo_manifest: Path, nfpm_config: Path) -> str:
    expected = version_from_tag(tag)
    manifests = (
        (cargo_manifest, "capglyph"),
        (
            cargo_manifest.parent / "crates/capglyph-server/Cargo.toml",
            "capglyph-server",
        ),
    )
    for manifest, name in manifests:
        actual = cargo_package_version(manifest, name)
        if actual != expected:
            raise MetadataError(
                f"Cargo/tag version mismatch for {name}: tag={expected}, Cargo={actual}"
            )

    try:
        config_text = nfpm_config.read_text(encoding="utf-8")
    except OSError as error:
        raise MetadataError(
            f"cannot read nFPM config {nfpm_config}: {error}"
        ) from error
    for key, expected_value in EXPECTED_CONFIG_FIELDS.items():
        actual = yaml_scalar(config_text, key)
        if actual != expected_value:
            raise MetadataError(
                f"nFPM config {key!r} must be {expected_value!r}, got {actual!r}"
            )
    if config_text.count("__CAPGLYPH_SOURCE__") != 2:
        raise MetadataError(
            "nFPM config must contain exactly two controlled package sources"
        )
    return expected


def render_config(
    tag: str,
    target_arch: str,
    source: Path,
    cargo_manifest: Path,
    nfpm_config: Path,
    output: Path,
) -> None:
    version = validate_contract(tag, cargo_manifest, nfpm_config)
    if target_arch not in ARCHITECTURES:
        raise MetadataError(f"unsupported target architecture: {target_arch!r}")
    if not source.is_file():
        raise MetadataError(f"package source does not exist: {source}")
    text = nfpm_config.read_text(encoding="utf-8")
    replacements = {
        "__CAPGLYPH_ARCH__": ARCHITECTURES[target_arch]["deb"],
        "__CAPGLYPH_VERSION__": version,
        "__CAPGLYPH_SOURCE__": str(source.resolve()),
    }
    expected_counts = {
        "__CAPGLYPH_ARCH__": 1,
        "__CAPGLYPH_VERSION__": 1,
        "__CAPGLYPH_SOURCE__": 2,
    }
    for token, value in replacements.items():
        if text.count(token) != expected_counts[token]:
            raise MetadataError(
                f"unexpected nFPM template token count for {token}: "
                f"expected {expected_counts[token]}, got {text.count(token)}"
            )
        text = text.replace(token, json.dumps(value))
    output.write_text(text, encoding="utf-8")


def ar_members(path: Path) -> dict[str, bytes]:
    data = path.read_bytes()
    if not data.startswith(b"!<arch>\n"):
        raise MetadataError(f"not a Debian ar archive: {path}")
    members: dict[str, bytes] = {}
    offset = 8
    while offset < len(data):
        header = data[offset : offset + 60]
        if len(header) != 60 or header[58:60] != b"`\n":
            raise MetadataError(f"malformed ar member in {path}")
        name = header[:16].decode("ascii").strip().rstrip("/")
        try:
            size = int(header[48:58].decode("ascii").strip())
        except ValueError as error:
            raise MetadataError(f"invalid ar member size in {path}") from error
        offset += 60
        members[name] = data[offset : offset + size]
        offset += size + (size % 2)
    return members


def decompress_tar(name: str, data: bytes) -> bytes:
    if name.endswith(".gz"):
        return gzip.decompress(data)
    if name.endswith((".xz", ".lzma")):
        return lzma.decompress(data)
    if name.endswith(".tar"):
        return data
    if name.endswith(".zst"):
        process = subprocess.run(
            ["zstd", "--decompress", "--stdout"],
            input=data,
            capture_output=True,
            check=False,
        )
        if process.returncode != 0:
            raise MetadataError(
                f"cannot decompress {name}: {process.stderr.decode().strip()}"
            )
        return process.stdout
    raise MetadataError(f"unsupported control archive compression: {name}")


def parse_fields(text: str) -> dict[str, str]:
    fields: dict[str, str] = {}
    for line in text.splitlines():
        if not line or line[0].isspace() or ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key] = value.strip()
    return fields


def deb_metadata(path: Path) -> dict[str, str]:
    members = ar_members(path)
    control_name = next(
        (name for name in members if name.startswith("control.tar")), None
    )
    if control_name is None:
        raise MetadataError(f"Debian package has no control archive: {path}")
    control_tar = decompress_tar(control_name, members[control_name])
    with tarfile.open(fileobj=io.BytesIO(control_tar), mode="r:") as archive:
        member = next(
            (
                entry
                for entry in archive.getmembers()
                if entry.name.lstrip("./") == "control"
            ),
            None,
        )
        if member is None:
            raise MetadataError(f"Debian package has no control file: {path}")
        extracted = archive.extractfile(member)
        if extracted is None:
            raise MetadataError(f"cannot read Debian control file: {path}")
        return parse_fields(extracted.read().decode("utf-8"))


def rpm_header(data: bytes, offset: int) -> tuple[dict[int, str], int]:
    if data[offset : offset + 3] != b"\x8e\xad\xe8" or data[offset + 3] != 1:
        raise MetadataError("malformed RPM header")
    index_count, store_size = struct.unpack_from(">II", data, offset + 8)
    if index_count > 100_000 or store_size > len(data):
        raise MetadataError("unreasonable RPM header sizes")
    indexes_start = offset + 16
    store_start = indexes_start + index_count * 16
    end = store_start + store_size
    if end > len(data):
        raise MetadataError("truncated RPM header")
    values: dict[int, str] = {}
    for index in range(index_count):
        tag, value_type, value_offset, count = struct.unpack_from(
            ">IIII", data, indexes_start + index * 16
        )
        if value_type == 6 and count == 1 and value_offset < store_size:
            start = store_start + value_offset
            stop = data.find(b"\0", start, end)
            if stop < 0:
                raise MetadataError("unterminated RPM string")
            values[tag] = data[start:stop].decode("utf-8")
    return values, end


def rpm_metadata(path: Path) -> dict[str, str]:
    data = path.read_bytes()
    if len(data) < 112 or data[:4] != b"\xed\xab\xee\xdb":
        raise MetadataError(f"not an RPM package: {path}")
    _, signature_end = rpm_header(data, 96)
    main_offset = (signature_end + 7) & ~7
    values, _ = rpm_header(data, main_offset)
    required = {1000: "name", 1001: "version", 1002: "release", 1022: "arch"}
    missing = [label for tag, label in required.items() if tag not in values]
    if missing:
        raise MetadataError(f"RPM metadata missing {', '.join(missing)}: {path}")
    return {label: values[tag] for tag, label in required.items()}


def arch_metadata(path: Path) -> dict[str, str]:
    try:
        with tarfile.open(path, mode="r:*") as archive:
            member = archive.getmember(".PKGINFO")
            extracted = archive.extractfile(member)
            if extracted is None:
                raise MetadataError(f"cannot read Arch .PKGINFO: {path}")
            text = extracted.read().decode("utf-8")
    except (tarfile.TarError, KeyError):
        process = subprocess.run(
            ["tar", "--zstd", "-xOf", str(path), ".PKGINFO"],
            capture_output=True,
            text=True,
            check=False,
        )
        if process.returncode != 0:
            raise MetadataError(f"cannot read Arch .PKGINFO: {process.stderr.strip()}")
        text = process.stdout
    fields: dict[str, str] = {}
    for line in text.splitlines():
        if " = " in line:
            key, value = line.split(" = ", 1)
            fields[key] = value
    return fields


def require_fields(kind: str, actual: dict[str, str], expected: dict[str, str]) -> None:
    for key, expected_value in expected.items():
        actual_value = actual.get(key)
        if actual_value != expected_value:
            raise MetadataError(
                f"{kind} metadata mismatch for {key}: expected {expected_value!r}, "
                f"got {actual_value!r}"
            )


def validate_packages(
    tag: str, target_arch: str, deb: Path, rpm: Path, arch: Path
) -> None:
    version = version_from_tag(tag)
    if target_arch not in ARCHITECTURES:
        raise MetadataError(f"unsupported target architecture: {target_arch!r}")
    architecture = ARCHITECTURES[target_arch]
    require_fields(
        "Debian",
        deb_metadata(deb),
        {
            "Package": "capglyph",
            "Version": f"{version}-1",
            "Architecture": architecture["deb"],
        },
    )
    require_fields(
        "RPM",
        rpm_metadata(rpm),
        {
            "name": "capglyph",
            "version": version,
            "release": "1",
            "arch": architecture["rpm"],
        },
    )
    require_fields(
        "Arch",
        arch_metadata(arch),
        {"pkgname": "capglyph", "pkgver": f"{version}-1", "arch": architecture["arch"]},
    )


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)
    contract = commands.add_parser(
        "contract", help="validate tag, Cargo, and nFPM inputs"
    )
    contract.add_argument("--tag", required=True)
    contract.add_argument("--cargo-manifest", required=True, type=Path)
    contract.add_argument("--nfpm-config", required=True, type=Path)
    render = commands.add_parser("render", help="render a validated nFPM config")
    render.add_argument("--tag", required=True)
    render.add_argument("--target-arch", required=True, choices=sorted(ARCHITECTURES))
    render.add_argument("--source", required=True, type=Path)
    render.add_argument("--cargo-manifest", required=True, type=Path)
    render.add_argument("--nfpm-config", required=True, type=Path)
    render.add_argument("--output", required=True, type=Path)
    packages = commands.add_parser("packages", help="inspect generated native packages")
    packages.add_argument("--tag", required=True)
    packages.add_argument("--target-arch", required=True, choices=sorted(ARCHITECTURES))
    packages.add_argument("--deb", required=True, type=Path)
    packages.add_argument("--rpm", required=True, type=Path)
    packages.add_argument("--arch", required=True, type=Path)
    return root


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "contract":
            version = validate_contract(args.tag, args.cargo_manifest, args.nfpm_config)
            print(f"validated release contract: {version}")
        elif args.command == "render":
            render_config(
                args.tag,
                args.target_arch,
                args.source,
                args.cargo_manifest,
                args.nfpm_config,
                args.output,
            )
            print(f"rendered validated nFPM config: {args.output}")
        else:
            validate_packages(args.tag, args.target_arch, args.deb, args.rpm, args.arch)
            print(f"validated deb/rpm/arch metadata: {args.tag} {args.target_arch}")
    except (MetadataError, OSError, subprocess.SubprocessError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
