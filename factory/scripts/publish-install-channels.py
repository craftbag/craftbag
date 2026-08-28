#!/usr/bin/env python3
"""Pack combined craftbag + craftbag-mcp archives and emit Homebrew/Scoop metadata.

Release CI must call `dist build`, then this script. Do not use cargo build
in tag workflows (compile-once lock).
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
)

UNIX_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
REQUIRED_UNIX_TARGETS = (
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
)

WINDOWS_TARGET = "x86_64-pc-windows-msvc"
HOMEPAGE = "https://github.com/craftbag/craftbag"
TAP_REPO = "craftbag/homebrew-tap"
BUCKET_REPO = "craftbag/scoop-bucket"


def log(phase: str, msg: str) -> None:
    print(f"{phase}: {msg}", flush=True)


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def write_sha256_sidecar(archive: Path) -> Path:
    sidecar = Path(str(archive) + ".sha256")
    sidecar.write_text(f"{sha256_file(archive)}  {archive.name}\n", encoding="utf-8")
    return sidecar


def archive_suffix(target: str) -> str:
    if target == WINDOWS_TARGET:
        return ".zip"
    return ".tar.xz"


def combined_name(target: str) -> str:
    return f"craftbag-{target}{archive_suffix(target)}"


def app_archive_name(app: str, target: str) -> str:
    return f"{app}-{target}{archive_suffix(target)}"


def find_archive(root: Path, name: str) -> Path:
    matches = sorted(p for p in root.rglob(name) if p.is_file())
    if not matches:
        raise SystemExit(f"missing archive {name} under {root}")
    return matches[0]


def extract_member(archive: Path, member_names: tuple[str, ...], dest: Path) -> Path:
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as zf:
            names = zf.namelist()
            for want in member_names:
                for name in names:
                    if Path(name).name == want:
                        dest.parent.mkdir(parents=True, exist_ok=True)
                        dest.write_bytes(zf.read(name))
                        dest.chmod(0o755)
                        return dest
    else:
        with tarfile.open(archive, "r:*") as tf:
            for want in member_names:
                for member in tf.getmembers():
                    if Path(member.name).name == want and member.isfile():
                        extracted = tf.extractfile(member)
                        if extracted is None:
                            continue
                        dest.parent.mkdir(parents=True, exist_ok=True)
                        dest.write_bytes(extracted.read())
                        dest.chmod(0o755)
                        return dest
    raise SystemExit(f"{archive} has none of {member_names}")


def write_combined_archive(staging: Path, dest: Path, target: str) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if target == WINDOWS_TARGET:
        with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as zf:
            for path in sorted(staging.iterdir()):
                zf.write(path, path.name)
        return
    with tarfile.open(dest, "w:xz") as tf:
        for path in sorted(staging.iterdir()):
            tf.add(path, arcname=path.name)


def pack_target(artifacts: Path, out_dir: Path, target: str) -> Path:
    cli = find_archive(artifacts, app_archive_name("craftbag-cli", target))
    mcp = find_archive(artifacts, app_archive_name("craftbag-mcp", target))
    with tempfile.TemporaryDirectory(prefix="craftbag-pack-") as tmp:
        staging = Path(tmp)
        if target == WINDOWS_TARGET:
            extract_member(cli, ("craftbag.exe",), staging / "craftbag.exe")
            extract_member(mcp, ("craftbag-mcp.exe",), staging / "craftbag-mcp.exe")
        else:
            extract_member(cli, ("craftbag",), staging / "craftbag")
            extract_member(mcp, ("craftbag-mcp",), staging / "craftbag-mcp")
        dest = out_dir / combined_name(target)
        write_combined_archive(staging, dest, target)
    write_sha256_sidecar(dest)
    return dest


def pack_all(artifacts: Path, out_dir: Path) -> list[Path]:
    log("PLAN", f"pack combined archives from {artifacts}")
    built: list[Path] = []
    for target in TARGETS:
        try:
            find_archive(artifacts, app_archive_name("craftbag-cli", target))
        except SystemExit:
            log("WAIT", f"skip {target}: no craftbag-cli archive")
            continue
        dest = pack_target(artifacts, out_dir, target)
        log("OK", f"{dest.name} sha256={sha256_file(dest)}")
        built.append(dest)
    if not built:
        raise SystemExit("no combined archives packed")
    log("DONE", f"packed {len(built)} archive(s)")
    return built


def hashes_from_dir(out_dir: Path) -> dict[str, str]:
    hashes: dict[str, str] = {}
    for target in TARGETS:
        path = out_dir / combined_name(target)
        if path.is_file():
            hashes[target] = sha256_file(path)
    return hashes


def normalize_version(version: str) -> str:
    return version[1:] if version.startswith("v") else version


def render_homebrew_formula(version: str, hashes: dict[str, str]) -> str:
    ver = normalize_version(version)
    missing = [t for t in REQUIRED_UNIX_TARGETS if t not in hashes]
    if missing:
        raise SystemExit(f"Homebrew formula needs hashes for {missing}")

    def block(target: str, indent: str) -> str:
        url = f"{HOMEPAGE}/releases/download/v{ver}/{combined_name(target)}"
        return (
            f'{indent}url "{url}"\n'
            f'{indent}sha256 "{hashes[target]}"\n'
        )

    linux_arm = ""
    if "aarch64-unknown-linux-gnu" in hashes:
        linux_arm = (
            "    on_arm do\n"
            + block("aarch64-unknown-linux-gnu", "      ")
            + "    end\n"
        )

    return f"""class Craftbag < Formula
  desc "Discover and load Agent Skills for CLI and MCP hosts"
  homepage "{HOMEPAGE}"
  version "{ver}"
  license "Apache-2.0 OR MIT"

  on_macos do
    on_arm do
{block("aarch64-apple-darwin", "      ").rstrip()}
    end
    on_intel do
{block("x86_64-apple-darwin", "      ").rstrip()}
    end
  end

  on_linux do
{linux_arm.rstrip()}
    on_intel do
{block("x86_64-unknown-linux-gnu", "      ").rstrip()}
    end
  end

  def install
    bin.install "craftbag"
    bin.install "craftbag-mcp"
  end

  test do
    assert_match version.to_s, shell_output("#{{bin}}/craftbag --version")
    assert_match version.to_s, shell_output("#{{bin}}/craftbag-mcp --version")
  end
end
"""


def render_scoop_manifest(version: str, hashes: dict[str, str]) -> str:
    ver = normalize_version(version)
    if WINDOWS_TARGET not in hashes:
        raise SystemExit(f"Scoop manifest needs hash for {WINDOWS_TARGET}")
    url = f"{HOMEPAGE}/releases/download/v{ver}/{combined_name(WINDOWS_TARGET)}"
    body = {
        "version": ver,
        "description": "Discover and load Agent Skills for CLI and MCP hosts",
        "homepage": HOMEPAGE,
        "license": "MIT|Apache-2.0",
        "architecture": {
            "64bit": {
                "url": url,
                "hash": hashes[WINDOWS_TARGET],
            }
        },
        "bin": ["craftbag.exe", "craftbag-mcp.exe"],
        "checkver": "github",
        "autoupdate": {
            "architecture": {
                "64bit": {
                    "url": f"{HOMEPAGE}/releases/download/v$version/{combined_name(WINDOWS_TARGET)}"
                }
            }
        },
    }
    return json.dumps(body, indent=4) + "\n"


def write_homebrew(path: Path, version: str, hashes: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_homebrew_formula(version, hashes), encoding="utf-8")
    log("OK", f"wrote {path}")


def write_scoop(path: Path, version: str, hashes: dict[str, str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(render_scoop_manifest(version, hashes), encoding="utf-8")
    log("OK", f"wrote {path}")


def make_tiny_unix_archive(path: Path, names: tuple[str, ...]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(path, "w:xz") as tf:
        for name in names:
            data = b"bin:" + name.encode()
            info = tarfile.TarInfo(name=name)
            info.size = len(data)
            info.mode = 0o755
            tf.addfile(info, io.BytesIO(data))


def make_tiny_zip(path: Path, names: tuple[str, ...]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as zf:
        for name in names:
            zf.writestr(name, b"bin:" + name.encode())


def self_test() -> int:
    log("PLAN", "publish-install-channels self-test")
    with tempfile.TemporaryDirectory(prefix="craftbag-dist-self-") as tmp:
        root = Path(tmp)
        artifacts = root / "artifacts"
        out = root / "out"
        unix = "x86_64-unknown-linux-gnu"
        win = WINDOWS_TARGET
        make_tiny_unix_archive(
            artifacts / app_archive_name("craftbag-cli", unix), ("craftbag",)
        )
        make_tiny_unix_archive(
            artifacts / app_archive_name("craftbag-mcp", unix), ("craftbag-mcp",)
        )
        make_tiny_zip(
            artifacts / app_archive_name("craftbag-cli", win), ("craftbag.exe",)
        )
        make_tiny_zip(
            artifacts / app_archive_name("craftbag-mcp", win), ("craftbag-mcp.exe",)
        )
        packed = pack_all(artifacts, out)
        names = {p.name for p in packed}
        if combined_name(unix) not in names or combined_name(win) not in names:
            log("FAIL", f"packed names {names}")
            return 1
        with tarfile.open(out / combined_name(unix), "r:xz") as tf:
            members = {Path(m.name).name for m in tf.getmembers() if m.isfile()}
        if members != {"craftbag", "craftbag-mcp"}:
            log("FAIL", f"unix archive members {members}")
            return 1
        with zipfile.ZipFile(out / combined_name(win)) as zf:
            znames = set(zf.namelist())
        if znames != {"craftbag.exe", "craftbag-mcp.exe"}:
            log("FAIL", f"windows archive members {znames}")
            return 1
        hashes = hashes_from_dir(out)
        # Formula needs every UNIX target; fill fixtures for the rest.
        for target in UNIX_TARGETS:
            hashes.setdefault(target, "a" * 64)
        formula = render_homebrew_formula("v0.1.0", hashes)
        if 'bin.install "craftbag-mcp"' not in formula:
            log("FAIL", "formula missing craftbag-mcp")
            return 1
        if "v0.1.0/craftbag-aarch64-apple-darwin.tar.xz" not in formula:
            log("FAIL", "formula missing macos arm url")
            return 1
        scoop = render_scoop_manifest("0.1.0", hashes)
        data = json.loads(scoop)
        if data["bin"] != ["craftbag.exe", "craftbag-mcp.exe"]:
            log("FAIL", f"scoop bin {data['bin']}")
            return 1
        if data["version"] != "0.1.0":
            log("FAIL", f"scoop version {data['version']}")
            return 1
        try:
            render_homebrew_formula("0.1.0", {unix: hashes[unix]})
        except SystemExit:
            pass
        else:
            log("FAIL", "formula accepted incomplete hashes")
            return 1
        macos_only = {
            "aarch64-apple-darwin": "b" * 64,
            "x86_64-apple-darwin": "c" * 64,
            "x86_64-unknown-linux-gnu": hashes[unix],
        }
        no_arm = render_homebrew_formula("0.1.0", macos_only)
        if "aarch64-unknown-linux-gnu" in no_arm:
            log("FAIL", "formula required optional linux arm")
            return 1
    if TAP_REPO != "craftbag/homebrew-tap" or BUCKET_REPO != "craftbag/scoop-bucket":
        log("FAIL", "repo constants drifted")
        return 1
    log("OK", "pack, formula, and scoop locks passed")
    log("DONE", "ok=true")
    log("NEXT", "none")
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--pack", action="store_true")
    parser.add_argument("--artifacts-dir", type=Path)
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--formula", type=Path)
    parser.add_argument("--scoop", type=Path)
    parser.add_argument("--version", default="")
    parser.add_argument("--hashes-dir", type=Path)
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    if args.self_test:
        return self_test()
    hashes: dict[str, str] = {}
    if args.pack:
        if args.artifacts_dir is None or args.out_dir is None:
            raise SystemExit("--pack needs --artifacts-dir and --out-dir")
        pack_all(args.artifacts_dir, args.out_dir)
        hashes = hashes_from_dir(args.out_dir)
    elif args.hashes_dir is not None:
        hashes = hashes_from_dir(args.hashes_dir)
    if args.formula is not None or args.scoop is not None:
        if not args.version:
            raise SystemExit("formula/scoop need --version")
        if not hashes:
            raise SystemExit("formula/scoop need --pack or --hashes-dir")
        if args.formula is not None:
            write_homebrew(args.formula, args.version, hashes)
        if args.scoop is not None:
            write_scoop(args.scoop, args.version, hashes)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except SystemExit as exc:
        if exc.code not in (0, None, 1, 2):
            log("FAIL", str(exc.code))
            log("DONE", "ok=false")
        raise
    except Exception as exc:
        log("FAIL", str(exc))
        log("DONE", "ok=false")
        sys.exit(1)
