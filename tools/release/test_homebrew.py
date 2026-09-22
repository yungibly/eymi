import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import homebrew


class HomebrewTests(unittest.TestCase):
    tag = "v0.1.0"

    def hashes(self):
        return {name: hashlib.sha256(name.encode()).hexdigest()
                for name in homebrew.archive_names(self.tag)}

    def manifest(self):
        return "".join(f"{digest}  {name}\n" for name, digest in self.hashes().items())

    def test_tag_and_manifest_reject_injection_missing_duplicates_and_unexpected_targets(self):
        for tag in ["0.1.0", "v0.1.0-rc1", "v01.1.0", "v0.1.0\n", "v../a", 'v0.1.0"']:
            with self.assertRaises(homebrew.ReleaseError):
                homebrew.version_from_tag(tag)
        manifest = self.manifest()
        for text in [manifest + manifest, manifest.splitlines()[0],
                     manifest + "0" * 64 + "  ../eymi.tar.gz\n",
                     manifest.replace("x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu")]:
            with self.assertRaises(homebrew.ReleaseError):
                homebrew.checksums(text, self.tag)

    def test_archive_bytes_are_verified_before_formula_generation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "SHA256SUMS").write_text(self.manifest())
            for name in homebrew.archive_names(self.tag):
                (root / name).write_text(name)
            self.assertEqual(homebrew.verified_checksums(root, self.tag), self.hashes())
            (root / homebrew.archive_names(self.tag)[0]).write_text("changed")
            with self.assertRaisesRegex(homebrew.ReleaseError, "Checksum mismatch"):
                homebrew.verified_checksums(root, self.tag)

    def test_formula_has_exact_platform_downloads_and_retains_all_notices(self):
        formula = homebrew.render_formula(self.tag, self.hashes())
        self.assertEqual(formula.count("releases/download/v0.1.0/"), 3)
        self.assertIn("depends_on arch: :x86_64", formula)
        self.assertIn('"third_party/iterm2-themes", "third_party/rust"', formula)
        self.assertIn('assert_equal source, (testpath/"note.md").read', formula)
        self.assertNotIn("unknown-linux-musl", formula)

    def test_update_is_idempotent_and_cannot_roll_back_or_replace_a_release(self):
        formula = homebrew.render_formula(self.tag, self.hashes())
        self.assertTrue(homebrew.replacement_needed(None, formula, self.tag))
        self.assertFalse(homebrew.replacement_needed(formula, formula, self.tag))
        self.assertFalse(homebrew.replacement_needed(formula.replace('version "0.1.0"',
                                                                   'version "0.2.0"'),
                                                     formula, self.tag))
        with self.assertRaisesRegex(homebrew.ReleaseError, "already released"):
            homebrew.replacement_needed(formula + "\n", formula, self.tag)

    def test_update_rejects_formula_code_changes_before_any_network_access(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "eymi.rb"
            path.write_text(homebrew.render_formula(self.tag, self.hashes()) + '\nsystem "bad"\n')
            with patch.object(homebrew, "request_bytes") as request:
                with self.assertRaisesRegex(homebrew.ReleaseError, "exact generated"):
                    homebrew.update_tap(path, self.tag)
                request.assert_not_called()

    def test_fetch_rejects_drafts_before_creating_files(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "release"
            with patch.object(homebrew, "request_bytes", return_value=b'{"tag_name":"v0.1.0","draft":true}'):
                with self.assertRaisesRegex(homebrew.ReleaseError, "published"):
                    homebrew.fetch_published(self.tag, output)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
