"""Focused release integrity tests; no network, credentials, or real releases."""
import gzip
import io
import json
from pathlib import Path
import struct
import tarfile
import tempfile
import unittest

import package


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'Cargo.toml').write_text('[package]\nname="eymi"\nversion="0.1.0"\n')
        (self.root / 'Cargo.lock').write_text('locked dependency bytes\n')
        for name in ['README.md', 'LICENSE', *[f'{package.THEMES}/{name}' for name in
                     ['LICENSE', 'CREDITS.md', 'README.md', 'additional-licenses.json', 'licenses/extra.txt']]]:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('Exact original contents: ' + name)
        self.target = package.TARGETS[0]
        self.binary = self.root / 'binary'
        self.binary.write_bytes(b'\xcf\xfa\xed\xfe' + struct.pack('<I', 0x0100000C) + b'\0' * 24)
        self.notices = self.root / 'notices'
        (self.notices / 'example-1.0').mkdir(parents=True)
        (self.notices / 'README.md').write_text('Actual upstream dependency notices\n')
        grant = b'Original dependency license grant\n'
        (self.notices / 'example-1.0/LICENSE').write_bytes(grant)
        self.manifest = {'target': self.target, 'cargo_lock_sha256': package.digest((self.root / 'Cargo.lock').read_bytes()),
                         'packages': [{'name': 'example', 'version': '1.0', 'license': 'MIT',
                                       'license_files': ['example-1.0/LICENSE'],
                                       'files': [{'path': 'example-1.0/LICENSE', 'sha256': package.digest(grant)}]}]}
        self.write_manifest()

    def write_manifest(self):
        (self.notices / 'manifest.json').write_text(json.dumps(self.manifest))

    def pack(self, directory='dist'):
        return package.package(self.root, self.binary, self.notices, self.root / directory,
                               'v0.1.0', self.target, 1234567890)

    def test_deterministic_archive_preserves_complete_notices_and_executable(self):
        first, second = self.pack(), self.pack('second')
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            self.assertEqual(archive.getmember('eymi').mode, 0o755)
            self.assertEqual(archive.extractfile('eymi').read(), self.binary.read_bytes())
            for name, data in package.tree(self.root / package.THEMES).items():
                self.assertEqual(archive.extractfile(package.THEMES + '/' + name).read(), data)
            self.assertEqual(archive.extractfile('third_party/rust/example-1.0/LICENSE').read(),
                             (self.notices / 'example-1.0/LICENSE').read_bytes())
            self.assertTrue(all(m.uid == m.gid == 0 and m.mtime == 1234567890 for m in archive))

    def test_existing_asset_is_never_overwritten(self):
        archive = self.pack()
        before = archive.read_bytes()
        with self.assertRaises(FileExistsError):
            self.pack()
        self.assertEqual(archive.read_bytes(), before)

    def test_tag_and_missing_root_license_stop_packaging(self):
        with self.assertRaises(ValueError):
            package.version(self.root, 'v9.9.9')
        (self.root / 'LICENSE').unlink()
        with self.assertRaises(ValueError):
            self.pack()
        self.assertFalse((self.root / 'dist').exists())

    def test_wrong_architecture_and_target_notice_bundle_are_rejected(self):
        self.binary.write_bytes(b'\xcf\xfa\xed\xfe' + struct.pack('<I', 0x01000007))
        with self.assertRaisesRegex(ValueError, 'architecture'):
            self.pack()
        self.binary.write_bytes(b'\xcf\xfa\xed\xfe' + struct.pack('<I', 0x0100000C))
        self.manifest['target'] = package.TARGETS[1]
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'different platform'):
            self.pack()

    def test_stale_lock_and_missing_license_grant_are_rejected(self):
        self.manifest['cargo_lock_sha256'] = 'stale'
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'Cargo.lock'):
            self.pack()
        self.manifest['cargo_lock_sha256'] = package.digest((self.root / 'Cargo.lock').read_bytes())
        self.manifest['packages'][0]['license_files'] = []
        self.write_manifest()
        with self.assertRaisesRegex(ValueError, 'license grant'):
            self.pack()

    def test_notice_changes_and_symlinks_are_rejected(self):
        (self.notices / 'example-1.0/LICENSE').write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            self.pack()
        (self.root / package.THEMES / 'linked.txt').symlink_to(self.binary)
        with self.assertRaisesRegex(ValueError, 'symlink'):
            self.pack()

    def test_checksums_require_exact_targets_and_reverify_lock_and_theme_contents(self):
        archive = self.pack()
        directory = archive.parent
        expected = package.checksum_text(self.root, directory, 'v0.1.0', [self.target])
        self.assertEqual(expected, f'{package.digest(archive.read_bytes())}  {archive.name}\n')
        with self.assertRaisesRegex(ValueError, 'missing'):
            package.checksum_text(self.root, directory, 'v0.1.0', package.TARGETS)
        with self.assertRaisesRegex(ValueError, 'distinct'):
            package.checksum_text(self.root, directory, 'v0.1.0', [self.target, self.target])
        (directory / 'wrong.tar.gz').write_bytes(b'unrelated')
        with self.assertRaisesRegex(ValueError, 'unexpected'):
            package.checksum_text(self.root, directory, 'v0.1.0', [self.target])
        (directory / 'wrong.tar.gz').unlink()
        (self.root / 'Cargo.lock').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'Cargo.lock'):
            package.checksum_text(self.root, directory, 'v0.1.0', [self.target])

    def test_duplicate_and_unsafe_archive_members_are_rejected(self):
        original = self.pack()
        for name in ['eymi', '../outside']:
            output = self.root / ('duplicate.tar.gz' if name == 'eymi' else 'unsafe.tar.gz')
            raw = gzip.decompress(original.read_bytes())
            with tarfile.open(fileobj=io.BytesIO(raw)) as source, tarfile.open(output, 'w:gz') as destination:
                for member in source:
                    destination.addfile(member, source.extractfile(member))
                item = tarfile.TarInfo(name)
                item.mode, item.size = 0o755 if name == 'eymi' else 0o644, 1
                destination.addfile(item, io.BytesIO(b'x'))
            with self.assertRaisesRegex(ValueError, 'Duplicate|Unsafe'):
                package.verify_archive(self.root, output, self.target)


if __name__ == '__main__':
    unittest.main()
