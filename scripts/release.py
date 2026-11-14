"""Build, smoke-test and publish metadata for atelier's three release targets."""

import argparse
import hashlib
import re
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path
from string import Template

ROOT = Path(__file__).resolve().parent.parent
TARGETS = ("darwin-arm64", "linux-arm64", "linux-x64")
REPOSITORY = "https://github.com/wrkhse/atelier"
# Releases contain only a native binary and the license, each at most 128 MiB.
MAX_FILE_BYTES = 128 * 1024 * 1024
MEMBERS = ("bin/atelier", "share/atelier/LICENSE")


def version(root: Path = ROOT) -> str:
    """Read the one version inherited by every crate."""
    with (root / "Cargo.toml").open("rb") as source:
        value = tomllib.load(source)["workspace"]["package"]["version"]
    if not isinstance(value, str) or not re.fullmatch(
        r"[0-9]+[.][0-9]+[.][0-9]+(-rc[.][0-9]+)?", value
    ):
        raise ValueError("invalid workspace version")
    return value


def artifact_name(release: str, target: str) -> str:
    if target not in TARGETS:
        raise ValueError(f"unknown target: {target}")
    if not re.fullmatch(r"[0-9]+[.][0-9]+[.][0-9]+(-rc[.][0-9]+)?", release):
        raise ValueError(f"invalid version: {release}")
    return f"atelier-{release}-{target}.tar.gz"


def package(binary: Path, target: str, out: Path, root: Path = ROOT) -> Path:
    """Package only the binary and license; reject symlinks and oversized input."""
    name = artifact_name(version(root), target)
    sources = (binary, root / "LICENSE")
    for source in sources:
        if source.is_symlink() or not source.is_file():
            raise ValueError(f"not a regular file: {source}")
        if not 0 < source.stat().st_size <= MAX_FILE_BYTES:
            raise ValueError(f"file size outside release limit: {source}")
    out.mkdir(parents=True, exist_ok=True)
    destination = out / name
    with tarfile.open(destination, "w:gz") as archive:
        for source, name in zip(sources, MEMBERS, strict=True):
            info = archive.gettarinfo(str(source), arcname=name)
            info.uid = info.gid = 0
            info.uname = info.gname = ""
            info.mode = 0o755 if name == "bin/atelier" else 0o644
            with source.open("rb") as stream:
                archive.addfile(info, stream)
    return destination


def sums(folder: Path, release: str) -> str:
    """Hash a nonempty subset of this release's targets, in stable order."""
    paths = sorted(folder.glob("*.tar.gz"))
    expected = {artifact_name(release, target) for target in TARGETS}
    if not paths or len(paths) > len(TARGETS):
        raise ValueError("expected one to three release archives")
    lines = []
    for path in paths:
        if path.name not in expected or path.is_symlink():
            raise ValueError(f"unexpected archive: {path.name}")
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        lines.append(f"{digest}  {path.name}\n")
    text = "".join(lines)
    (folder / "SHA256SUMS").write_text(text)
    return text


def formula(release: str, checksums: str) -> str:
    """Render a formula only from a complete, unambiguous checksum manifest."""
    if not re.fullmatch(r"[0-9]+[.][0-9]+[.][0-9]+(-rc[.][0-9]+)?", release):
        raise ValueError("invalid formula version")
    parsed = {}
    for line in checksums.splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([^ ]+)", line)
        if match is None or match[2] in parsed:
            raise ValueError("invalid or duplicate checksum")
        parsed[match[2]] = match[1]
    expected = {artifact_name(release, target) for target in TARGETS}
    if set(parsed) != expected:
        raise ValueError("checksums must name exactly the three release targets")
    blocks = []
    for os_name, targets in (("macos", ("darwin-arm64",)), ("linux", TARGETS[1:])):
        lines = [f"  on_{os_name} do"]
        if os_name == "macos":
            lines.append("    depends_on arch: :arm64")
        for target in targets:
            cpu = "arm" if target.endswith("arm64") else "intel"
            name = artifact_name(release, target)
            lines.extend(
                [
                    f"    on_{cpu} do",
                    f'      url "{REPOSITORY}/releases/download/v{release}/{name}"',
                    f'      sha256 "{parsed[name]}"',
                    "    end",
                ]
            )
        lines.append("  end")
        blocks.append("\n".join(lines))
    platforms = "\n\n".join(blocks)
    template = (ROOT / "scripts/atelier.rb.in").read_text()
    return Template(template).substitute(
        repository=REPOSITORY, release=release, platforms=platforms
    )


def smoke(archive_path: Path, release: str) -> None:
    """Unpack a bounded, known layout, then execute the actual packaged binary."""
    with tempfile.TemporaryDirectory(prefix="atelier-smoke-") as directory:
        prefix = Path(directory)
        with tarfile.open(archive_path, "r:gz") as archive:
            for expected in MEMBERS:
                info = archive.next()
                if info is None or info.name != expected or not info.isfile():
                    raise ValueError(f"expected regular archive member: {expected}")
                if not 0 < info.size <= MAX_FILE_BYTES:
                    raise ValueError("archive member exceeds release size limit")
                destination = prefix / expected
                destination.parent.mkdir(parents=True, exist_ok=True)
                stream = archive.extractfile(info)
                if stream is None:
                    raise ValueError("archive member has no contents")
                with stream:
                    destination.write_bytes(stream.read(MAX_FILE_BYTES))
                destination.chmod(info.mode & 0o777)
            if archive.next() is not None:
                raise ValueError("unexpected extra archive member")
        binary = prefix / "bin/atelier"
        result = subprocess.run(
            [binary, "--version"],
            capture_output=True,
            text=True,
            timeout=10,
            check=True,
        )
        if result.stdout != f"atelier {release}\n" or result.stderr:
            raise ValueError(f"incorrect packaged version: {result}")
        subprocess.run(
            [binary, "--help"], capture_output=True, timeout=10, check=True
        )
    print(f"{archive_path}: packaged version and help passed")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("version")
    pack = sub.add_parser("package")
    pack.add_argument("--binary", required=True, type=Path)
    pack.add_argument("--target", required=True, choices=TARGETS)
    pack.add_argument("--out", type=Path, default=Path("dist"))
    checksum = sub.add_parser("sums")
    checksum.add_argument("folder", type=Path)
    brew = sub.add_parser("formula")
    brew.add_argument("release")
    brew.add_argument("checksums", type=Path)
    brew.add_argument("--out", required=True, type=Path)
    test = sub.add_parser("smoke")
    test.add_argument("archive", type=Path)
    args = parser.parse_args()
    match args.command:
        case "version":
            print(version())
        case "package":
            print(package(args.binary, args.target, args.out))
        case "sums":
            print(sums(args.folder, version()), end="")
        case "formula":
            text = formula(args.release, args.checksums.read_text())
            args.out.write_text(text)
        case "smoke":
            smoke(args.archive, version())


if __name__ == "__main__":
    main()
