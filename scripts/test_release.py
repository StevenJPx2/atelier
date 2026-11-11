"""Contract tests use real tar archives and real subprocesses, without network."""

import hashlib
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

import release


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix="atelier-release-test-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        (self.root / "Cargo.toml").write_text(
            '[workspace.package]\nversion = "0.1.0-rc.1"\n'
        )
        (self.root / "LICENSE").write_text("MIT license fixture\n")
        self.binary = self.root / "atelier"
        self.binary.write_text(
            '#!/bin/sh\nif [ "$1" = "--version" ]; then '
            "printf 'atelier 0.1.0-rc.1\\n'; fi\n"
        )
        self.binary.chmod(0o755)
        self.out = self.root / "dist"

    def checksums(self):
        return "".join(
            f"{str(index) * 64}  {release.artifact_name('0.1.0-rc.1', target)}\n"
            for index, target in enumerate(release.TARGETS)
        )

    def pack(self):
        return release.package(self.binary, "darwin-arm64", self.out, self.root)

    def test_workspace_version_and_invalid_version(self):
        self.assertEqual(release.version(self.root), "0.1.0-rc.1")
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "bad"')
        with self.assertRaisesRegex(ValueError, "invalid workspace version"):
            release.version(self.root)

    def test_exact_layout_contents_modes_and_hash(self):
        path = self.pack()
        self.assertEqual(path.name, "atelier-0.1.0-rc.1-darwin-arm64.tar.gz")
        with tarfile.open(path) as archive:
            self.assertEqual(archive.getnames(), list(release.MEMBERS))
            self.assertEqual(archive.getmember("bin/atelier").mode, 0o755)
            self.assertEqual(archive.getmember("share/atelier/LICENSE").mode, 0o644)
            self.assertEqual(
                archive.extractfile("bin/atelier").read(), self.binary.read_bytes()
            )
            self.assertEqual(
                archive.extractfile("share/atelier/LICENSE").read(),
                (self.root / "LICENSE").read_bytes(),
            )
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        expected = f"{digest}  {path.name}\n"
        self.assertEqual(release.sums(self.out, "0.1.0-rc.1"), expected)
        self.assertEqual((self.out / "SHA256SUMS").read_text(), expected)

    def test_package_rejects_invalid_inputs(self):
        for target in ("windows", "../escape"):
            with self.subTest(target=target), self.assertRaises(ValueError):
                release.package(self.binary, target, self.out, self.root)
        for version in ("bad", "../escape", '0.1.0"'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                release.artifact_name(version, "darwin-arm64")
        self.binary.unlink()
        with self.assertRaisesRegex(ValueError, "not a regular file"):
            self.pack()
        self.binary.symlink_to(self.root / "LICENSE")
        with self.assertRaisesRegex(ValueError, "not a regular file"):
            self.pack()
        self.binary.unlink()
        self.binary.touch()
        with self.assertRaisesRegex(ValueError, "file size"):
            self.pack()
        with self.binary.open("wb") as stream:
            stream.truncate(release.MAX_FILE_BYTES + 1)
        with self.assertRaisesRegex(ValueError, "file size"):
            self.pack()

    def test_checksum_rejects_empty_unknown_and_symlink_archives(self):
        self.out.mkdir()
        with self.assertRaises(ValueError):
            release.sums(self.out, "0.1.0-rc.1")
        unknown = self.out / "unknown.tar.gz"
        unknown.touch()
        with self.assertRaisesRegex(ValueError, "unexpected archive"):
            release.sums(self.out, "0.1.0-rc.1")
        unknown.unlink()
        link = self.out / release.artifact_name("0.1.0-rc.1", "linux-x64")
        link.symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, "unexpected archive"):
            release.sums(self.out, "0.1.0-rc.1")

    def test_formula_has_all_targets_and_the_service(self):
        formula = release.formula("0.1.0-rc.1", self.checksums())
        for index, target in enumerate(release.TARGETS):
            name = release.artifact_name("0.1.0-rc.1", target)
            self.assertIn(f'/{name}"\n      sha256 "{str(index) * 64}"', formula)
        for expected in (
            "class Atelier < Formula",
            "depends_on arch: :arm64",
            'bin.install "bin/atelier"',
            'run [opt_bin/"atelier", "daemon"]',
            "keep_alive true",
            "environment_variables PATH: std_service_path_env",
            'log_path var/"log/atelier.log"',
            'error_log_path var/"log/atelier.log"',
            "#{bin}/atelier --version",
        ):
            self.assertIn(expected, formula)
        self.assertNotIn("darwin-x64", formula)
        path = self.root / "atelier.rb"
        path.write_text(formula)
        subprocess.run(
            ["ruby", "-c", path], check=True, capture_output=True, timeout=10
        )

    def test_formula_lets_homebrew_read_the_version_from_the_url(self):
        formula = release.formula("0.1.0-rc.1", self.checksums())
        self.assertNotIn('  version "', formula)

    def test_formula_rejects_bad_manifests_and_versions(self):
        lines = self.checksums().splitlines()
        for text in (
            "",
            "not a checksum",
            "\n".join(lines[1:]),
            self.checksums() + lines[0],
            self.checksums().replace("0" * 64, "g" * 64),
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                release.formula("0.1.0-rc.1", text)
        with self.assertRaisesRegex(ValueError, "invalid formula version"):
            release.formula("../unsafe", self.checksums())

    def test_smoke_executes_packaged_binary_and_rejects_wrong_version(self):
        path = self.pack()
        release.smoke(path, "0.1.0-rc.1")
        with self.assertRaisesRegex(ValueError, "incorrect packaged version"):
            release.smoke(path, "0.2.0")
        self.binary.write_text("#!/bin/sh\nexit 1\n")
        with self.assertRaises(subprocess.CalledProcessError):
            release.smoke(self.pack(), "0.1.0-rc.1")

    def test_smoke_rejects_unexpected_members_and_links(self):
        for name, kind, size in (
            ("../escape", tarfile.REGTYPE, 1),
            ("bin/atelier", tarfile.SYMTYPE, 0),
            ("bin/atelier", tarfile.REGTYPE, release.MAX_FILE_BYTES + 1),
        ):
            path = self.root / "bad.tar.gz"
            info = tarfile.TarInfo(name)
            info.type = kind
            info.size = size
            # A header alone suffices: rejection happens before reading the body.
            with tarfile.open(path, "w:gz") as archive:
                archive.fileobj.write(info.tobuf())
            with (
                self.subTest(name=name, kind=kind, size=size),
                self.assertRaises(ValueError),
            ):
                release.smoke(path, "0.1.0-rc.1")

        path = self.root / "extra.tar.gz"
        with tarfile.open(path, "w:gz") as archive:
            archive.add(self.binary, arcname="bin/atelier")
            archive.add(self.root / "LICENSE", arcname="share/atelier/LICENSE")
            archive.add(self.root / "LICENSE", arcname="extra")
        with self.assertRaisesRegex(ValueError, "unexpected extra"):
            release.smoke(path, "0.1.0-rc.1")

    def test_command_line_failure_and_version(self):
        script = release.ROOT / "scripts/release.py"
        result = subprocess.run(
            [sys.executable, script, "version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(result.stdout, f"{release.version()}\n")
        result = subprocess.run(
            [
                sys.executable,
                script,
                "package",
                "--target",
                "bad",
                "--binary",
                self.binary,
            ],
            check=False,
            capture_output=True,
            timeout=10,
        )
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
