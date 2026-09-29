"""Exercise offline catalog validation against isolated repository snapshots."""
from copy import deepcopy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))


class CatalogMetadataTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for name in ('plugins', 'docs/pages', 'src', 'schemas', '.claude-plugin', '.agents', 'api'):
            shutil.copytree(ROOT / name, self.root / name)
        for name in ('README.md', 'catalog-info.json', 'Cargo.toml', 'package.json', '.mcp.json',
                     'requirements-catalog.txt', 'openapi.json', 'Cargo.lock', 'package-lock.json'):
            shutil.copyfile(ROOT / name, self.root / name)
        (self.root / 'scripts').mkdir()
        shutil.copyfile(ROOT / 'scripts/check_catalog.py', self.root / 'scripts/check_catalog.py')
        self.data = json.loads((self.root / 'catalog-info.json').read_text())

    def validate(self, data=None, **kwargs):
        import check_catalog
        (self.root / 'catalog-info.json').write_text(json.dumps(self.data if data is None else data))
        return check_catalog.validate(self.root, **kwargs)

    def rejected(self, data):
        import check_catalog
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(data, check_review=False)

    def test_current_metadata_and_review_receipt(self):
        self.validate(refresh_review=True)
        self.validate()

    def test_schema_identity_and_unknown_fields(self):
        for field, value in [('schemaVersion', 2), ('schemaVersion', True),
                             ('pluginId', 'invented'), ('capability_count', 9)]:
            data = deepcopy(self.data)
            data[field] = value
            with self.subTest(field=field, value=value):
                self.rejected(data)
        data = deepcopy(self.data)
        data['platforms']['slack']['invented'] = True
        self.rejected(data)

    def test_examples_require_real_capabilities_and_all_fields(self):
        for ref in ('skill:invented', 'command:invented', 'hook:invented',
                    'mcp_server:invented', 'mcp_tool:invented'):
            data = deepcopy(self.data)
            data['examples'][0]['capability_refs'] = [ref]
            self.rejected(data)
        for field in ('id', 'title', 'platform', 'input_or_trigger', 'expected_behavior',
                      'prerequisites', 'sources', 'capability_refs', 'evidence'):
            data = deepcopy(self.data)
            del data['examples'][0][field]
            with self.subTest(field=field):
                self.rejected(data)
        data = deepcopy(self.data)
        data['examples'].pop(0)
        self.rejected(data)
        data = deepcopy(self.data)
        data['examples'].append(deepcopy(data['examples'][0]))
        self.rejected(data)

    def test_legacy_source_alias_cannot_hide_an_unvalidated_path(self):
        data = deepcopy(self.data)
        data['examples'][0]['source'] = {'path': '../../outside.md', 'format': 'markdown',
                                         'mode': 'section', 'heading_path': ['Missing']}
        self.rejected(data)

    def test_nested_extension_sources_cannot_bypass_validation(self):
        data = deepcopy(self.data)
        data['examples'][0]['additional_evidence'] = {'sources': [{'path': '../outside.md'}]}
        self.rejected(data)
        data['examples'][0]['additional_evidence'] = {'sources': [
            {'path': 'README.md', 'format': 'markdown', 'mode': 'section',
             'heading_path': ['Missing heading']}]}
        self.rejected(data)
        data['examples'][0]['additional_evidence'] = {'sources': [{'path': 'docs/extra.md'}]}
        (self.root / 'docs/extra.md').write_text('Reviewed supporting evidence.\n')
        self.validate(data, refresh_review=True)
        (self.root / 'docs/extra.md').write_text('Changed evidence.\n')
        import check_catalog
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(data)

    def test_native_hook_needs_a_targeted_note_as_well_as_an_example(self):
        import check_catalog
        hooks = self.root / 'plugins/trakt-mcp/hooks/hooks.json'
        hooks.parent.mkdir()
        hooks.write_text(json.dumps({'hooks': {'SessionStart': [{'hooks': [
            {'type': 'command', 'command': 'echo example'}]}]}}))
        self.data['examples'][0]['capability_refs'].append('hook:SessionStart:0:0')
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(refresh_review=True)
        note = {'target': {'path': 'plugins/trakt-mcp/hooks/hooks.json',
                           'pointer': '/hooks/SessionStart/0/hooks/0'},
                'description': 'Illustrative test hook; never executed by validation.',
                'sources': [self.data['overview']]}
        self.data['hooks'] = [note]
        self.validate(refresh_review=True)
        self.validate()
        self.data['hooks'].append(deepcopy(note))
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(refresh_review=True)

    def test_platform_evidence_and_invented_component_notes(self):
        for status in ('documented', 'unsupported'):
            data = deepcopy(self.data)
            data['platforms']['slack']['status'] = status
            self.rejected(data)
            data['platforms']['slack']['sources'] = [{'path': 'missing.md', 'required': False}]
            self.rejected(data)
        for field, value in [('mcpServers', {'invented': {}}),
                             ('hooks', [{'target': {'path': 'README.md', 'pointer': '/hooks/fake'}}])]:
            data = deepcopy(self.data)
            data[field] = value
            self.rejected(data)

    def test_paths_cannot_escape_or_resolve_to_directories(self):
        (self.root / 'outside-link').symlink_to(ROOT / 'README.md')
        for path in ('../README.md', '/etc/passwd', 'C:/secret', 'https://example.test',
                     'docs\\pages\\index.html', 'outside-link', 'docs', 'missing.html'):
            data = deepcopy(self.data)
            data['overview']['path'] = path
            with self.subTest(path=path):
                self.rejected(data)

    def test_html_selectors_must_match_exactly_one_nonempty_element(self):
        for selector in ('#missing', 'section', '[', 'p:contains('):
            data = deepcopy(self.data)
            data['overview']['selector'] = selector
            self.rejected(data)
        (self.root / 'docs/pages/index.html').write_text('<p class="lead"></p>')
        self.rejected(self.data)

    def test_markdown_hierarchy_fences_setext_and_ambiguity(self):
        import check_catalog
        path = self.root / 'README.md'
        path.write_text('# One\n## Same\nFirst\n# Two\nSame\n----\nSecond\n```md\n# Fake\n```\n')
        selector = {'path': 'README.md', 'format': 'markdown', 'mode': 'section',
                    'heading_path': ['Two', 'Same']}
        check_catalog.resolve_source(self.root, selector)
        for headings in (['Same'], ['Fake'], ['Missing']):
            with self.subTest(headings=headings), self.assertRaises(check_catalog.CatalogError):
                check_catalog.resolve_source(self.root, {**selector, 'heading_path': headings})
        path.write_text('# Empty\n# Next\nContent\n')
        with self.assertRaises(check_catalog.CatalogError):
            check_catalog.resolve_source(self.root, {**selector, 'heading_path': ['Empty']})
        path.write_text('# Empty\n<!-- no visible evidence -->\n')
        with self.assertRaises(check_catalog.CatalogError):
            check_catalog.resolve_source(self.root, {**selector, 'heading_path': ['Empty']})

    def test_removed_skill_cannot_be_replaced_by_compatibility_copies(self):
        shutil.copytree(self.root / 'plugins/trakt-mcp/skills', self.root / 'skills')
        shutil.rmtree(self.root / 'plugins/trakt-mcp/skills/what-to-watch')
        self.rejected(self.data)

    def test_tool_inventory_rejects_adapter_drift_and_comment_invention(self):
        path = self.root / 'src/mcp/protocol.rs'
        text = path.read_text()
        path.write_text(text.replace('tool("trakt_search"', 'tool(dynamic_name'))
        self.rejected(self.data)
        path.write_text(text + '\n// tool("invented", "Fake", "Fake", x)\n')
        data = deepcopy(self.data)
        data['examples'][0]['capability_refs'] = ['mcp_tool:invented']
        self.rejected(data)

    def test_non_helper_tools_and_alternate_return_construction_fail_closed(self):
        import check_catalog
        path = self.root / 'src/mcp/protocol.rs'
        text = path.read_text()
        for replacement in (
            'json!({"tools":[json!({"name":"hidden_tool"}),',
            'json!({"tools":[other_helper(),',
            'json!({"tools":vec![',
        ):
            path.write_text(text.replace('json!({"tools":[', replacement))
            with self.subTest(replacement=replacement), self.assertRaises(check_catalog.CatalogError):
                self.validate(refresh_review=True)
        for changed in (
            text.replace('    json!({"tools":[', '    return json!({"tools":[]});\n    json!({"tools":['),
            text.replace('    json!({"tools":[', '    let mut result = json!({"tools":[')
                .replace('    ]})\n}', '    ]});\n    result["tools"].as_array_mut().unwrap().push(other_helper());\n    result\n}'),
        ):
            path.write_text(changed)
            with self.subTest(changed=changed[-150:]), self.assertRaises(check_catalog.CatalogError):
                self.validate(refresh_review=True)

    def test_nested_changelogs_require_selector_and_stay_fingerprinted(self):
        import check_catalog
        self.validate(refresh_review=True)
        path = self.root / 'docs/releases/CHANGELOG.md'
        path.parent.mkdir(parents=True)
        path.write_text('# Changes\nReviewed release notes.\n')
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(refresh_review=True)
        self.data['changelog'] = {'path': 'docs/releases/CHANGELOG.md', 'format': 'markdown',
                                  'mode': 'section', 'heading_path': ['Changes']}
        self.validate(refresh_review=True)
        path.write_text('# Changes\nUpdated release notes.\n')
        with self.assertRaises(check_catalog.CatalogError):
            self.validate()
        other = self.root / 'plugins/trakt-mcp/releases/history.md'
        other.parent.mkdir(parents=True)
        other.write_text('# History\nMore notes.\n')
        self.data['changelog'] = None
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(refresh_review=True)

    def test_nested_changelog_name_variants_require_review(self):
        import check_catalog
        self.validate(refresh_review=True)
        for filename in ('CHANGELOG-2026.md', 'project-changelog.md', 'changes_2026.html',
                         'project-HISTORY.md'):
            path = self.root / 'docs/releases' / filename
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('# Release notes\nChanged behavior.\n')
            with self.subTest(filename=filename), self.assertRaises(check_catalog.CatalogError):
                self.validate()
            with self.assertRaises(check_catalog.CatalogError):
                self.validate(refresh_review=True)
            path.unlink()

    def test_changelog_discovery_covers_other_project_directories(self):
        import check_catalog
        self.validate(refresh_review=True)
        for name in ('api/trakt/CHANGELOG.md', 'scripts/releases/changes.md',
                     'other/project-changelog.html', 'api/trakt/CHANGELOG',
                     'other/CHANGES', 'other/HISTORY', 'api/trakt/CHANGELOG.txt',
                     'docs/releases/HISTORY.rst', 'other/changes.markdown'):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('Project release notes.\n')
            with self.subTest(name=name), self.assertRaises(check_catalog.CatalogError):
                self.validate(refresh_review=True)
            path.unlink()
        for name in ('target/dependency/CHANGELOG.md', 'node_modules/library/CHANGELOG.md',
                     '.venv/lib/CHANGELOG.md'):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('Dependency release notes.\n')
        self.validate()

    def test_missing_changelog_field_is_not_the_explicit_fallback(self):
        data = deepcopy(self.data)
        del data['changelog']
        self.rejected(data)

    def test_source_changes_additions_and_metadata_edits_require_review(self):
        import check_catalog
        self.validate(refresh_review=True)
        paths = ['README.md', 'plugins/trakt-mcp/skills/what-to-watch/SKILL.md',
                 'src/mcp/handlers.rs', 'docs/pages/new-guide.html', 'CHANGELOG.md',
                 'requirements-catalog.txt', 'api/trakt/catalog.json',
                 'openapi.json', 'scripts/check_catalog.py', 'Cargo.lock', 'package-lock.json']
        for name in paths:
            path = self.root / name
            original = path.read_bytes() if path.exists() else None
            path.write_bytes((original or b'') + b'\nChanged\n')
            with self.subTest(path=name), self.assertRaises(check_catalog.CatalogError):
                self.validate()
            if original is None:
                path.unlink()
            else:
                path.write_bytes(original)
        self.data['platforms']['slack']['notes'] = 'Reviewed new wording.'
        with self.assertRaises(check_catalog.CatalogError):
            self.validate()
        self.validate(refresh_review=True)
        self.validate()

    def test_invalid_metadata_cannot_refresh_review(self):
        import check_catalog
        self.validate(refresh_review=True)
        receipt = (self.root / 'catalog-sources.lock.json').read_bytes()
        self.data['overview']['selector'] = '#missing'
        with self.assertRaises(check_catalog.CatalogError):
            self.validate(refresh_review=True)
        self.assertEqual(receipt, (self.root / 'catalog-sources.lock.json').read_bytes())

    def test_malformed_duplicate_json_and_diagnostics_do_not_echo_values(self):
        for raw in ('{"private": "SECRET_VALUE",',
                    '{"schemaVersion":1,"schemaVersion":2}',
                    '{"schemaVersion":1,"pluginId":"SECRET_VALUE"}'):
            (self.root / 'catalog-info.json').write_text(raw)
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/check_catalog.py'),
                                     '--root', str(self.root)], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Catalog validation failed', result.stderr)
            self.assertNotIn('SECRET_VALUE', result.stdout + result.stderr)
            self.assertNotIn(str(self.root), result.stdout + result.stderr)

    def test_release_gate_failure_stops_build_and_publication(self):
        workflow = yaml.load((ROOT / '.github/workflows/release.yml').read_text(), Loader=yaml.BaseLoader)
        steps = workflow['jobs']['release']['steps']
        checkout = next(s for s in steps if s.get('uses', '').startswith('actions/checkout@'))
        self.assertEqual(checkout['with']['ref'], '${{ github.sha }}')
        gate_index = next(i for i, s in enumerate(steps) if 'scripts/check_catalog.py' in s.get('run', ''))
        gate = steps[gate_index]
        self.assertNotIn('if', gate)
        self.assertNotIn('continue-on-error', gate)
        self.assertNotIn('--refresh-reviewed-sources', gate['run'])
        build_index = next(i for i, s in enumerate(steps) if 'scripts/package_release.py' in s.get('run', ''))
        publish_index = next(i for i, s in enumerate(steps) if 'gh release create' in s.get('run', ''))
        self.assertLess(gate_index, build_index)
        self.assertLess(build_index, publish_index)
        # Execute the actual gate command in a broken snapshot. Subsequent marker
        # commands represent build/publish, neither of which may be reached.
        shutil.copytree(ROOT / 'scripts', self.root / 'scripts', dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('__pycache__'))
        (self.root / 'catalog-info.json').write_text('{"schemaVersion": 999}')
        command = next(line for line in gate['run'].splitlines() if 'scripts/check_catalog.py' in line)
        result = subprocess.run(['sh', '-ec', command + '\ntouch built\ntouch published\n'],
                                cwd=self.root, capture_output=True, text=True,
                                env={**os.environ,
                                     'PATH': str(Path(sys.executable).parent) + ':' + os.environ['PATH']})
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Catalog validation failed', result.stderr)
        self.assertFalse((self.root / 'built').exists())
        self.assertFalse((self.root / 'published').exists())


if __name__ == '__main__':
    unittest.main()
