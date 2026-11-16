import importlib.util
from pathlib import Path
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("release_manifest", ROOT / "scripts/release-manifest.py")
MANIFEST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MANIFEST)


class ReleaseManifestTests(unittest.TestCase):
    def test_repository_declares_compatible_core(self):
        self.assertEqual(MANIFEST.read_required_core(ROOT / "Cargo.toml"), "0.21.6")

    def test_missing_and_malformed_floors_refuse(self):
        for declaration in ["", 'requires_core = "v0.21.6"', 'requires_core = "0.21"',
                            'requires_core = "0.21.6\nINJECT"', 'requires_core = 216',
                            'requires_core = "0.21.6"\nrequires_core = "0.21.7"']:
            with self.subTest(declaration=declaration), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / "Cargo.toml"
                path.write_text("[workspace.metadata.release]\n" + declaration, encoding="utf-8")
                with self.assertRaises(ValueError):
                    MANIFEST.read_required_core(path)

    def test_plain_version_is_accepted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "Cargo.toml"
            path.write_text('[workspace.metadata.release]\nrequires_core = "9.8.7"\n', encoding="utf-8")
            self.assertEqual(MANIFEST.read_required_core(path), "9.8.7")


if __name__ == "__main__":
    unittest.main()
