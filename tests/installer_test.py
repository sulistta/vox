"""Exercise installer transactions in isolated prefixes, with injected I/O failures."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / "packaging/linux/install.sh"


class InstallerTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="vox-installer-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "bundle"
        (self.bundle / "runtime").mkdir(parents=True)
        # Manifest verification has its own suite; this fixture isolates file transactions.
        self.executable(self.bundle / "runtime/node", "#!/bin/sh\nexit 0\n")
        self.executable(self.bundle / "run.sh", "#!/bin/sh\nprintf 'new\\n'\n")
        for name in ("verify-bundle-manifest.mjs", "sbom.cdx.json"):
            (self.bundle / name).write_text("{}")
        self.prefix = self.root / "current"
        self.prefix.mkdir()
        (self.prefix / "version").write_text("old")
        self.shims = self.root / "shims"
        self.shims.mkdir()
        self.env = dict(os.environ, HOME=str(self.root), PATH=f"{self.shims}:{os.environ['PATH']}")

    @staticmethod
    def executable(path, text):
        path.write_text(text)
        path.chmod(0o755)

    def install(self):
        return subprocess.run(
            ["bash", str(INSTALLER), "--bundle", str(self.bundle), "--prefix", str(self.prefix)],
            env=self.env, capture_output=True, text=True, timeout=15,
        )

    def assert_old_preserved(self):
        self.assertEqual((self.prefix / "version").read_text(), "old")
        self.assertEqual(list(self.root.glob(".vox-install.*")), [])
        self.assertEqual(list(self.root.glob(".vox-previous.*")), [])

    def test_install_and_reinstall(self):
        for _ in range(2):
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((self.prefix / "lib/vox/sbom.cdx.json").read_text(), "{}")
            launched = subprocess.run([str(self.prefix / "bin/vox")], capture_output=True, text=True, timeout=5)
            self.assertEqual(launched.returncode, 0, launched.stderr)
            self.assertEqual(launched.stdout, "new\n")
        self.assertEqual(list(self.root.glob(".vox-previous.*")), [])

    def test_partial_copy_failure_preserves_old(self):
        cp = shutil.which("cp")
        self.executable(self.shims / "cp", f'#!/bin/sh\n"{cp}" "$@"\nexit 1\n')
        self.assertNotEqual(self.install().returncode, 0)
        self.assert_old_preserved()

    def test_activation_failure_restores_old(self):
        mv = shutil.which("mv")
        self.executable(self.shims / "mv", f'''#!/bin/sh
case "$2" in */.vox-install.*) exit 1 ;; esac
exec "{mv}" "$@"
''')
        self.assertNotEqual(self.install().returncode, 0)
        self.assert_old_preserved()

    def test_failed_restore_keeps_recoverable_backup(self):
        mv = shutil.which("mv")
        self.executable(self.shims / "mv", f'''#!/bin/sh
case "$2" in */.vox-install.*|*/.vox-previous.*/prefix) exit 1 ;; esac
exec "{mv}" "$@"
''')
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        backups = list(self.root.glob(".vox-previous.*/prefix/version"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_text(), "old")
        self.assertIn("restauração falhou", result.stderr)


if __name__ == "__main__":
    unittest.main()
