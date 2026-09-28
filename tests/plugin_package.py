#!/usr/bin/env python3
"""Negative package cases catch containment, credential and dependency regressions."""
from pathlib import Path
import shutil
import json
import sys
import tempfile
import unittest
import zipfile
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
from check_plugin import validate, ROOT, SKILLS
from package_plugin import package

class PackageBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(prefix='trakt-package-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)/'plugin'
        shutil.copytree(ROOT/'plugins/trakt-mcp',self.root)

    def rejected(self):
        with self.assertRaises(ValueError):validate(self.root,repository_checks=False)

    def test_clean_copy_without_checkout_dependencies(self):
        validate(self.root,repository_checks=False)

    def test_external_symlink(self):
        (self.root/'assets/outside.svg').symlink_to('/etc/passwd')
        self.rejected()

    def test_missing_skill_resource(self):
        p=self.root/'skills/find-title/SKILL.md'
        p.write_text(p.read_text()+'\nSee [procedure](missing.md).\n')
        self.rejected()

    def test_escape_from_individual_skill(self):
        for name in SKILLS:
            with self.subTest(skill=name):
                p=self.root/'skills'/name/'SKILL.md'
                original=p.read_text()
                try:
                    p.write_text(original+'\nSee [outside](../../README.md).\n')
                    self.rejected()
                finally:
                    p.write_text(original)

    def test_missing_workflow(self):
        shutil.rmtree(self.root/'skills/manage-library')
        self.rejected()

    def test_missing_standalone_dependency_metadata(self):
        (self.root/'skills/lists-and-watchlist/agents/openai.yaml').unlink()
        self.rejected()

    def test_mismatched_standalone_dependency(self):
        p=self.root/'skills/upcoming-releases/agents/openai.yaml'
        p.write_text(p.read_text().replace('https://trakt.swacktech.com/mcp',
                                        'https://unrelated.example/mcp'))
        self.rejected()

    def test_stale_platform_interface(self):
        p=self.root/'.codex-plugin/plugin.json'
        data=json.loads(p.read_text())
        data['interface']['capabilities']=['Read']
        p.write_text(json.dumps(data))
        self.rejected()

    def test_undeclared_write_capability(self):
        for filename in ['plugin.json','.codex-plugin/plugin.json']:
            p=self.root/filename
            data=json.loads(p.read_text())
            interface=data['interface'] if 'interface' in data else data['extensions']['com.openai']['interface']
            interface['capabilities']=['Read']
            p.write_text(json.dumps(data))
        self.rejected()

    def test_accidental_environment_file(self):
        (self.root/'.env').write_text('DUMMY=value\n')
        self.rejected()

    def test_credential_pattern(self):
        (self.root/'leak.md').write_text('ghp_'+'x'*36)
        self.rejected()

    def test_lfs_pointer(self):
        (self.root/'assets/logo.png').write_text('version https://git-lfs.github.com/spec/v1\n')
        self.rejected()

    def test_mismatched_endpoint(self):
        p=self.root/'mcp.json'
        data=json.loads(p.read_text())
        data['mcpServers']['trakt']['url']='https://wrong.example/mcp'
        p.write_text(json.dumps(data))
        self.rejected()

class ArchiveContractTests(unittest.TestCase):
    def test_all_standalone_skills_and_deterministic_archives(self):
        with tempfile.TemporaryDirectory(prefix='trakt-archive-test-') as tmp:
            first,second=Path(tmp)/'first',Path(tmp)/'second'
            package(first)
            package(second)
            self.assertEqual((first/'SHA256SUMS').read_bytes(),(second/'SHA256SUMS').read_bytes())
            version=json.loads((ROOT/'plugins/trakt-mcp/plugin.json').read_text())['version']
            expected={f'{name}-{version}.zip' for name in SKILLS}
            expected|={f'trakt-mcp-{version}.zip',f'trakt-mcp-skills-{version}.zip'}
            self.assertEqual({p.name for p in first.glob('*.zip')},expected)
            for name in SKILLS:
                with self.subTest(skill=name),zipfile.ZipFile(first/f'{name}-{version}.zip') as artifact:
                    self.assertTrue(all(p.startswith(name+'/') for p in artifact.namelist()))
                    for member in ['SKILL.md','agents/openai.yaml','LICENSE']:
                        self.assertIn(name+'/'+member,artifact.namelist())
                    self.assertEqual(artifact.read(name+'/LICENSE'),(ROOT/'plugins/trakt-mcp/LICENSE').read_bytes())


if __name__=='__main__':unittest.main()
