#!/usr/bin/env python3
"""Rewrite the Chocolatey nuspec version and the x64 zip URL and checksum.

The Windows zip is the only architecture published today. Checksum text is
the cargo-dist ``.sha256`` file (hash, then optional filename).
"""

from __future__ import annotations

import argparse
import hashlib
import pathlib
import re
import sys
import tempfile

ASSET = "craftbag-x86_64-pc-windows-msvc.zip"
REPO = "https://github.com/craftbag/craftbag"


def parse_sha256_sidecar(path: pathlib.Path) -> str:
    token = path.read_text(encoding="utf-8").strip().split()[0].lower()
    if not re.fullmatch(r"[0-9a-f]{64}", token):
        raise SystemExit(f"bad sha256 in {path}")
    return token


def checksum_from_artifact(artifacts_dir: pathlib.Path) -> str:
    sidecar = artifacts_dir / f"{ASSET}.sha256"
    zip_path = artifacts_dir / ASSET
    sidecar_hash = parse_sha256_sidecar(sidecar) if sidecar.is_file() else ""
    if zip_path.is_file():
        digest = hashlib.sha256(zip_path.read_bytes()).hexdigest()
        if sidecar_hash and sidecar_hash != digest:
            raise SystemExit(f"checksum asset does not match {zip_path.name}")
        return digest
    if sidecar_hash:
        return sidecar_hash
    raise SystemExit(f"missing {ASSET} and its checksum in {artifacts_dir}")


def zip_url(version: str) -> str:
    return f"{REPO}/releases/download/v{version}/{ASSET}"


def render_install_script(version: str, checksum: str) -> str:
    url = zip_url(version)
    body = "\n".join(
        [
            "$ErrorActionPreference = 'Stop'",
            "$packageName = $env:ChocolateyPackageName",
            '$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"',
            f"$url64 = '{url}'",
            f"$checksum64 = '{checksum}'",
            "$packageArgs = @{",
            "    packageName    = $packageName",
            "    unzipLocation  = $toolsDir",
            "    url64bit       = $url64",
            "    checksum64     = $checksum64",
            "    checksumType64 = 'sha256'",
            "}",
            "Install-ChocolateyZipPackage @packageArgs",
            "",
        ]
    )
    return body


def render_nuspec(current: str, version: str) -> str:
    updated, n_ver = re.subn(
        r"<version>[^<]+</version>",
        f"<version>{version}</version>",
        current,
        count=1,
    )
    if n_ver != 1:
        raise SystemExit("nuspec is missing <version>")
    notes = f"{REPO}/releases/tag/v{version}"
    updated, n_notes = re.subn(
        r"<releaseNotes>[^<]+</releaseNotes>",
        f"<releaseNotes>{notes}</releaseNotes>",
        updated,
        count=1,
    )
    if n_notes != 1:
        raise SystemExit("nuspec is missing <releaseNotes>")
    if "<projectUrl>https://docs.rs/craftbag</projectUrl>" not in updated:
        raise SystemExit("nuspec projectUrl drifted")
    if "<projectSourceUrl>https://github.com/craftbag/craftbag</projectSourceUrl>" not in updated:
        raise SystemExit("nuspec projectSourceUrl drifted")
    if updated.count("<projectUrl>") != 1:
        raise SystemExit("nuspec must keep projectUrl different from projectSourceUrl")
    return updated


def write_package(package_dir: pathlib.Path, version: str, checksum: str) -> None:
    nuspec = package_dir / "craftbag.nuspec"
    script = package_dir / "tools" / "chocolateyInstall.ps1"
    script.parent.mkdir(parents=True, exist_ok=True)
    nuspec.write_text(render_nuspec(nuspec.read_text(encoding="utf-8"), version), encoding="utf-8")
    # PowerShell 5.1 parses UTF-8 install scripts only when a BOM is present.
    script.write_bytes(render_install_script(version, checksum).encode("utf-8-sig"))


def package_matches(package_dir: pathlib.Path, version: str, checksum: str) -> bool:
    nuspec = (package_dir / "craftbag.nuspec").read_text(encoding="utf-8")
    script = (package_dir / "tools" / "chocolateyInstall.ps1").read_bytes()
    expected = render_install_script(version, checksum).encode("utf-8-sig")
    return (
        f"<version>{version}</version>" in nuspec
        and f"/releases/tag/v{version}<" in nuspec
        and script == expected
    )


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = pathlib.Path(tmp)
        package = root / "chocolatey"
        tools = package / "tools"
        tools.mkdir(parents=True)
        (package / "craftbag.nuspec").write_text(
            "\n".join(
                [
                    '<package xmlns="http://schemas.microsoft.com/packaging/2015/06/nuspec.xsd">',
                    "  <metadata>",
                    "    <id>craftbag</id>",
                    "    <version>0.0.1</version>",
                    "    <projectUrl>https://docs.rs/craftbag</projectUrl>",
                    "    <projectSourceUrl>https://github.com/craftbag/craftbag</projectSourceUrl>",
                    "    <releaseNotes>https://github.com/craftbag/craftbag/releases/tag/v0.0.1</releaseNotes>",
                    "  </metadata>",
                    "</package>",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        checksum = "ab" * 32
        artifacts = root / "artifacts"
        artifacts.mkdir()
        (artifacts / f"{ASSET}.sha256").write_text(
            f"{checksum.upper()}  {ASSET}\n",
            encoding="utf-8",
        )
        write_package(package, "0.2.0", checksum_from_artifact(artifacts))
        if not package_matches(package, "0.2.0", checksum):
            raise SystemExit("self-test: rendered package does not match")
        script = (tools / "chocolateyInstall.ps1").read_bytes()
        if not script.startswith(b"\xef\xbb\xbf"):
            raise SystemExit("self-test: install script is missing the UTF-8 BOM")
        if b"urlArm64" in script:
            raise SystemExit("self-test: x64-only zip must not invent an arm64 URL")
        (artifacts / f"{ASSET}.sha256").write_text("zz\n", encoding="utf-8")
        try:
            checksum_from_artifact(artifacts)
        except SystemExit:
            pass
        else:
            raise SystemExit("self-test: bad checksum was accepted")
        zip_dir = root / "zip-artifacts"
        zip_dir.mkdir()
        blob = b"craftbag-zip"
        (zip_dir / ASSET).write_bytes(blob)
        digest = hashlib.sha256(blob).hexdigest()
        if checksum_from_artifact(zip_dir) != digest:
            raise SystemExit("self-test: zip without a sidecar was not hashed")
        (zip_dir / f"{ASSET}.sha256").write_text(
            f"{'cd' * 32}  {ASSET}\n",
            encoding="utf-8",
        )
        try:
            checksum_from_artifact(zip_dir)
        except SystemExit:
            pass
        else:
            raise SystemExit("self-test: mismatched sidecar was accepted")
    print("DONE: ok=true")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--version")
    parser.add_argument("--artifacts-dir", type=pathlib.Path)
    parser.add_argument("--package-dir", type=pathlib.Path, default=pathlib.Path("chocolatey"))
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not args.version or args.artifacts_dir is None:
        raise SystemExit("--version and --artifacts-dir are required")
    version = args.version[1:] if args.version.startswith("v") else args.version
    checksum = checksum_from_artifact(args.artifacts_dir)
    if args.check:
        if package_matches(args.package_dir, version, checksum):
            print(f"OK: chocolatey package matches {version}")
            return
        raise SystemExit(f"chocolatey package is stale for {version}")
    write_package(args.package_dir, version, checksum)
    print(f"OK: chocolatey package set to {version}")


if __name__ == "__main__":
    try:
        main()
    except BrokenPipeError:
        sys.exit(0)
