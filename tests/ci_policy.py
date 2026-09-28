"""Protect the PR-only testing and main-push-only deployment boundary."""
from copy import deepcopy
from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
DEPLOYMENTS = {'deploy.yml', 'docs.yml'}


def validate_policy(name, workflow):
    events = workflow.get('on', {})
    assert isinstance(events, dict), f'{name}: explicit trigger mapping required'
    if name in DEPLOYMENTS:
        assert set(events) == {'push'}, f'{name}: deployments must only run on push'
        assert events['push'].get('branches') == ['main'], f'{name}: main branch required'
        for job in workflow['jobs'].values():
            for step in job.get('steps', []):
                command = step.get('run', '')
                assert not re.search(
                    r'cargo (?:test|fmt|clippy)|test:integration|unittest|ruff check|'
                    r'check_(?:plugin|portable_schema|routes|prose)\.py|docs/check\.py|vale ',
                    command,
                ), f'{name}: test-only work belongs in PR checks'
    else:
        assert set(events) == {'pull_request'}, f'{name}: checks must only run on PRs'
        assert not events['pull_request'], f'{name}: all PRs need required checks'
        assert workflow.get('permissions') == {'contents': 'read'}, f'{name}: read-only token required'
        assert 'secrets.' not in str(workflow), f'{name}: PR checks must be credential-free'
        for job in workflow['jobs'].values():
            permissions = job.get('permissions', {})
            # CodeQL needs SARIF upload permission; it cannot deploy or modify source.
            allowed = {'contents': 'read', 'security-events': 'write'} if name == 'codeql.yml' else {'contents': 'read'}
            assert all(allowed.get(key) == value for key, value in permissions.items()), name
            assert 'environment' not in job, f'{name}: no PR deployment environment'
    for job in workflow['jobs'].values():
        assert job.get('runs-on') == 'ubuntu-24.04', f'{name}: use a standard pinned Ubuntu runner'
        for step in job.get('steps', []):
            action = step.get('uses', '')
            if action:
                assert re.fullmatch(r'[^@]+@[0-9a-f]{40}', action), f'{name}: pin action commit: {action}'


class WorkflowPolicyTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.workflows = {
            path.name: yaml.load(path.read_text(), Loader=yaml.BaseLoader)
            for path in (ROOT / '.github/workflows').glob('*.yml')
        }

    def test_all_workflows_follow_event_and_permission_policy(self):
        self.assertLessEqual(DEPLOYMENTS, self.workflows.keys())
        for name, workflow in self.workflows.items():
            with self.subTest(workflow=name):
                validate_policy(name, workflow)

    def test_checks_reject_push_and_manual_triggers(self):
        for event in ('push', 'workflow_dispatch', 'pull_request_target', 'schedule'):
            workflow = deepcopy(self.workflows['checks.yml'])
            workflow['on'][event] = {}
            with self.subTest(event=event), self.assertRaises(AssertionError):
                validate_policy('checks.yml', workflow)

    def test_deployment_rejects_pr_and_other_branches(self):
        workflow = deepcopy(self.workflows['deploy.yml'])
        workflow['on']['pull_request'] = {}
        with self.assertRaises(AssertionError):
            validate_policy('deploy.yml', workflow)
        workflow = deepcopy(self.workflows['deploy.yml'])
        workflow['on']['push']['branches'] = ['main', 'development']
        with self.assertRaises(AssertionError):
            validate_policy('deploy.yml', workflow)

    def test_pr_jobs_reject_credentials_and_write_tokens(self):
        for field, value in [('env', {'TOKEN': '${{ secrets.TOKEN }}'}),
                             ('permissions', {'contents': 'write'})]:
            workflow = deepcopy(self.workflows['checks.yml'])
            workflow['jobs']['rust-and-mcp'][field] = value
            with self.subTest(field=field), self.assertRaises(AssertionError):
                validate_policy('checks.yml', workflow)

    def test_codeql_can_upload_findings_but_cannot_write_source(self):
        workflow = deepcopy(self.workflows['codeql.yml'])
        validate_policy('codeql.yml', workflow)
        workflow['jobs']['analyze']['permissions']['contents'] = 'write'
        with self.assertRaises(AssertionError):
            validate_policy('codeql.yml', workflow)

    def test_deployments_reject_repeated_test_suites(self):
        workflow = deepcopy(self.workflows['deploy.yml'])
        workflow['jobs']['build-and-deploy']['steps'].append({'run': 'cargo test'})
        with self.assertRaises(AssertionError):
            validate_policy('deploy.yml', workflow)

    def test_validation_categories_remain_present(self):
        commands = '\n'.join(
            step.get('run', '')
            for name, workflow in self.workflows.items() if name not in DEPLOYMENTS
            for job in workflow['jobs'].values()
            for step in job.get('steps', [])
        )
        for required in ('cargo fmt --check', 'cargo test',
                         'cargo clippy --target wasm32-unknown-unknown -- -D warnings',
                         'ruff check', 'unittest discover', 'test:integration',
                         'scripts/check_plugin.py', 'scripts/check_portable_schema.py',
                         'skills-ref validate', 'docs/build.py', 'docs/check.py',
                         'scripts/check_prose.py', 'actionlint'):
            with self.subTest(category=required):
                self.assertIn(required, commands)


if __name__ == '__main__':
    unittest.main()
