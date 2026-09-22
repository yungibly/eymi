"""Notice coverage regressions, including real objc2 publication omissions."""
import json
from pathlib import Path
import tempfile
import unittest

import notices


class NoticeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.package = {'id': 'normal', 'name': 'normal', 'version': '1.0.0',
                        'manifest_path': str(self.root / 'Cargo.toml'), 'license': 'MIT',
                        'license_file': None, 'source': 'registry+example', 'authors': ['Actual Author']}
        (self.root / 'Cargo.toml').write_text('package')

    def test_normal_and_build_dependencies_exclude_development_only_nodes(self):
        packages = [{'id': name, 'name': name, 'version': '1'} for name in ['root', 'normal', 'build', 'dev']]
        metadata = {'packages': packages, 'resolve': {'root': 'root', 'nodes': [
            {'id': 'root', 'deps': [{'pkg': name, 'dep_kinds': [{'kind': kind}]} for name, kind in
                                    [('normal', None), ('build', 'build'), ('dev', 'dev')]]},
            *[{'id': name, 'deps': []} for name in ['normal', 'build', 'dev']]]}}
        self.assertEqual([p['name'] for p in notices.dependency_packages(metadata)], ['build', 'normal'])

    def test_full_mixed_license_notices_and_copyright_are_retained(self):
        self.package['license'] = '(MIT OR Apache-2.0) AND Unicode-3.0'
        for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-UNICODE', 'COPYRIGHT', 'README.md']:
            (self.root / name).write_text('Actual original: ' + name)
        files, grants, override = notices.notice_files(self.package, {})
        self.assertEqual(set(files), {'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-UNICODE', 'COPYRIGHT', 'README.md'})
        self.assertEqual(set(grants), {'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-UNICODE'})
        self.assertIsNone(override)
        metadata = {'packages': [self.package], 'resolve': {'root': 'root', 'nodes': [
            {'id': 'root', 'deps': [{'pkg': 'normal', 'dep_kinds': [{'kind': None}]}]},
            {'id': 'normal', 'deps': []}]}}
        output = self.root / 'bundle'
        manifest = notices.generate(metadata, 'target', output, 'lock')
        self.assertEqual(manifest['packages'][0]['license'], self.package['license'])
        self.assertEqual(manifest['packages'][0]['authors'], ['Actual Author'])
        self.assertEqual((output / 'normal-1.0.0/LICENSE-UNICODE').read_bytes(),
                         (self.root / 'LICENSE-UNICODE').read_bytes())
        with self.assertRaises(FileExistsError):
            notices.generate(metadata, 'target', output, 'lock')

    def test_source_code_readme_and_author_list_cannot_substitute_for_a_license_grant(self):
        for name in ['copying.rs', 'README.md', 'AUTHORS', 'COPYRIGHT']:
            (self.root / name).write_text('Supplemental data, not a license grant')
        with self.assertRaisesRegex(ValueError, 'No upstream license grant'):
            notices.notice_files(self.package, {})

    def test_pinned_foundation_override_covers_mit_and_ignores_copying_source(self):
        config = json.loads(notices.OVERRIDES.read_text())
        override = next(p for p in config['packages'] if p['name'] == 'objc2-foundation')
        self.package.update({k: override[k] for k in ['name', 'version', 'source']})
        (self.root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': override['vcs_sha1']}}))
        (self.root / 'copying.rs').write_text('Code, not a license')
        key = (override['name'], override['version'], override['source'])
        files, grants, actual = notices.notice_files(self.package, {key: override['checksum']})
        self.assertEqual(actual['selected_license'], 'MIT')
        self.assertEqual(grants, ['STANDARD-MIT.txt'])
        self.assertIn('UPSTREAM-LICENSE.md', files)
        self.assertNotIn('copying.rs', files)
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            notices.notice_files(self.package, {key: 'different-lock-checksum'})
        (self.root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'wrong-revision'}}))
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            notices.notice_files(self.package, {key: override['checksum']})

    def test_linked_or_parent_license_is_not_read_as_a_crate_notice(self):
        outside = self.root / 'outside.txt'
        outside.write_text('Unrelated text')
        (self.root / 'LICENSE').symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'linked'):
            notices.notice_files(self.package, {})


if __name__ == '__main__':
    unittest.main()
