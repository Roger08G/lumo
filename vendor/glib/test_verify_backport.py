"""Regression controls for the vendored source integrity gate; no network needed."""

import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest

sys.dont_write_bytecode = True
from verify_backport import source_file


class BackportVerificationTests(unittest.TestCase):
    def test_corrupt_archive_is_rejected_with_and_without_optimization(self):
        with tempfile.TemporaryDirectory(prefix="lumo-glib-verifier-") as directory:
            root = Path(directory).resolve()
            self.assertTrue(root.is_relative_to(Path(tempfile.gettempdir()).resolve()))
            archive = root / "corrupt.crate"
            # A valid tar container with altered content exercises the checksum
            # gate; merely invalid gzip would fail later even if checks vanished.
            with tarfile.open(archive, "w:gz") as output:
                payload = b"CORRUPT LICENSE CONTROL"
                entry = tarfile.TarInfo("glib-0.18.5/LICENSE")
                entry.size = len(payload)
                output.addfile(entry, io.BytesIO(payload))
            verifier = Path(__file__).with_name("verify_backport.py")
            modes = [([], {}), (["-O"], {}), ([], {"PYTHONOPTIMIZE": "1"})]
            for flags, environment in modes:
                with self.subTest(flags=flags, environment=environment):
                    result = subprocess.run(
                        [sys.executable, *flags, str(verifier), "--archive", str(archive)],
                        capture_output=True, text=True,
                        env={**os.environ, **environment}, timeout=30,
                    )
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("Archive mismatch", result.stderr)
                    self.assertNotIn("files verified", result.stdout)

    def test_source_path_cannot_escape_vendor_directory(self):
        with tempfile.TemporaryDirectory(prefix="lumo-glib-verifier-") as directory:
            root = Path(directory).resolve()
            self.assertTrue(root.is_relative_to(Path(tempfile.gettempdir()).resolve()))
            with self.assertRaisesRegex(ValueError, "Unsafe source path"):
                source_file(root, "../outside")
            with self.assertRaisesRegex(ValueError, "Unsafe source path"):
                source_file(root, "/outside")

    def test_source_symlink_is_rejected_before_following_it(self):
        with tempfile.TemporaryDirectory(prefix="lumo-glib-verifier-") as directory:
            root = Path(directory).resolve()
            self.assertTrue(root.is_relative_to(Path(tempfile.gettempdir()).resolve()))
            target = root / "target.txt"
            target.write_text("controlled test data", encoding="utf-8")
            link = root / "link.txt"
            try:
                link.symlink_to(target)
            except OSError as error:
                self.skipTest(f"Symlink creation is unavailable on this host: {error}")
            with self.assertRaisesRegex(ValueError, "symlinks are not allowed"):
                source_file(root, "link.txt")


if __name__ == "__main__":
    unittest.main()
