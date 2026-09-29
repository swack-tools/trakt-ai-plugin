"""Verify that release archives are self-contained for each plugin host."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / 'plugins/trakt-mcp'


class ReleasePackageTests(unittest.TestCase):
    def test_release_archives_have_host_specific_manifests_and_skills(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            subprocess.run(
                [sys.executable, str(ROOT / 'scripts/package_release.py'), str(out)],
                cwd=ROOT, check=True, capture_output=True, text=True,
            )
            expected_skills = {f'trakt-mcp/skills/{p.name}/SKILL.md'
                               for p in (PLUGIN / 'skills').iterdir() if p.is_dir()}
            manifests = {
                'claude': ('.claude-plugin/plugin.json', '.mcp.json'),
                'codex': ('plugin.json', 'mcp.json'),
            }
            sums = (out / 'SHA256SUMS').read_text().splitlines()
            self.assertEqual(len(sums), 2)
            for client, (manifest_path, mcp_path) in manifests.items():
                filename = f'trakt-mcp-{client}.zip'
                archive = out / filename
                expected_hash = next(line.split()[0] for line in sums if line.endswith(filename))
                self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(), expected_hash)
                with zipfile.ZipFile(archive) as zipped:
                    self.assertIsNone(zipped.testzip())
                    names = set(zipped.namelist())
                    self.assertIn(f'trakt-mcp/{manifest_path}', names)
                    self.assertIn(f'trakt-mcp/{mcp_path}', names)
                    self.assertTrue(expected_skills <= names)
                    self.assertEqual(client == 'claude',
                                     'trakt-mcp/.claude-plugin/plugin.json' in names)
                    self.assertNotIn('trakt-mcp/.codex-plugin/plugin.json', names)
                    manifest = json.loads(zipped.read(f'trakt-mcp/{manifest_path}'))
                    mcp = json.loads(zipped.read(f'trakt-mcp/{mcp_path}'))
                    self.assertEqual(manifest['name'], 'trakt-mcp')
                    self.assertEqual(manifest['version'], '2.0.0')
                    self.assertEqual(len(mcp['mcpServers']), 1)
                    self.assertTrue(mcp['mcpServers']['trakt']['url'].endswith('/mcp'))


if __name__ == '__main__':
    unittest.main()
