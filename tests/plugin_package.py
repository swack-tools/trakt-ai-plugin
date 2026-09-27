#!/usr/bin/env python3
"""Negative package cases catch containment, credential and dependency regressions."""
from pathlib import Path
import shutil
import json
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
from check_plugin import validate, ROOT

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
        p=self.root/'skills/find-title/SKILL.md'
        p.write_text(p.read_text()+'\nSee [outside](../../README.md).\n')
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

if __name__=='__main__':unittest.main()
