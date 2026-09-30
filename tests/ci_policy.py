"""Protect the PR-only testing and main-push-only deployment boundary."""
from copy import deepcopy
from pathlib import Path
import re
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[1]
DEPLOYMENTS = {'deploy.yml', 'docs.yml'}
RELEASES = {'release.yml'}

# This job alone can use the cross-repository credential. Keep the destination,
# dependency, execution surface, and token scope explicit in the policy.
MARKETPLACE_NOTIFICATION = {
    'name': 'Request marketplace refresh',
    'needs': 'release',
    'runs-on': 'ubuntu-24.04',
    'timeout-minutes': '5',
    'permissions': {},
    'steps': [{
        'name': 'Dispatch marketplace build',
        'env': {'GH_TOKEN': '${{ secrets.MARKETPLACE_DISPATCH_TOKEN }}'},
        'run': '''if [ -z "$GH_TOKEN" ]; then
  echo "::error::Configure MARKETPLACE_DISPATCH_TOKEN with Actions write access to swack-tools/ai-plugin-marketplace."
  exit 1
fi
gh workflow run pages.yml \\
  --repo swack-tools/ai-plugin-marketplace \\
  --ref main \\
  -f plugin_repository="$GITHUB_REPOSITORY" \\
  -f plugin_tag="$GITHUB_REF_NAME"
echo "Requested marketplace refresh for $GITHUB_REPOSITORY at $GITHUB_REF_NAME." >> "$GITHUB_STEP_SUMMARY"
''',
    }],
}


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
    elif name in RELEASES:
        assert set(events) == {'push'}, f'{name}: releases must only run for tag pushes'
        assert events['push'].get('tags') == ['v*'], f'{name}: version tags required'
        assert 'branches' not in events['push'], f'{name}: release must not run for branch pushes'
        assert workflow.get('permissions') == {'contents': 'write'}, f'{name}: release asset permission required'
        assert 'pull_request' not in events, f'{name}: PRs must not publish releases'
        remaining = deepcopy(workflow)
        notification = remaining['jobs'].pop('notify-marketplace', None)
        assert notification == MARKETPLACE_NOTIFICATION, f'{name}: narrowly scoped notification job required'
        assert set(remaining['jobs']) == {'release'}, f'{name}: unexpected release job'
        assert 'secrets.' not in str(remaining), f'{name}: publishing must use the scoped GitHub token only'
        for job in remaining['jobs'].values():
            assert job.get('permissions') == {'contents': 'write'}, f'{name}: scope write access to release job'
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
        assert 1 <= int(job.get('timeout-minutes', 0)) <= 30, f'{name}: bounded job timeout required'
        assert job.get('runs-on') == 'ubuntu-24.04', f'{name}: use a standard pinned Ubuntu runner'
        for step in job.get('steps', []):
            action = step.get('uses', '')
            if action.startswith('actions/checkout@'):
                assert str(step.get('with', {}).get('persist-credentials')).lower() == 'false', name
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

    def test_release_only_accepts_version_tag_pushes(self):
        workflow = deepcopy(self.workflows['release.yml'])
        validate_policy('release.yml', workflow)
        for mutation in (
            lambda w: w['on'].__setitem__('pull_request', {}),
            lambda w: w['on']['push'].__setitem__('branches', ['main']),
            lambda w: w['on']['push'].__setitem__('tags', ['*']),
            lambda w: w.__setitem__('permissions', {'contents': 'read'}),
            lambda w: w['jobs']['release'].__setitem__('permissions', {'contents': 'read'}),
        ):
            bad = deepcopy(workflow)
            mutation(bad)
            with self.subTest(workflow=bad), self.assertRaises(AssertionError):
                validate_policy('release.yml', bad)
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

    def test_release_notification_rejects_expanded_authority(self):
        for field, value in (
            ('permissions', {'contents': 'write'}),
            ('needs', []),
            ('if', 'always()'),
            ('env', {'TOKEN': '${{ secrets.OTHER_TOKEN }}'}),
            ('environment', 'production'),
        ):
            workflow = deepcopy(self.workflows['release.yml'])
            workflow['jobs']['notify-marketplace'][field] = value
            with self.subTest(field=field), self.assertRaises(AssertionError):
                validate_policy('release.yml', workflow)

    def test_release_notification_rejects_other_credentials_or_commands(self):
        for mutation in (
            lambda s: s['env'].__setitem__('GH_TOKEN', '${{ secrets.OTHER_TOKEN }}'),
            lambda s: s['env'].__setitem__('OTHER', '${{ secrets.OTHER_TOKEN }}'),
            lambda s: s.__setitem__('run', s['run'].replace('--ref main', '--ref development')),
            lambda s: s.__setitem__('run', s['run'].replace('pages.yml', 'other.yml')),
            lambda s: s.__setitem__('run', s['run'].replace('--repo swack-tools/ai-plugin-marketplace', '--repo other/repo')),
            lambda s: s.__setitem__('run', s['run'] + 'echo "$GH_TOKEN"\n'),
        ):
            workflow = deepcopy(self.workflows['release.yml'])
            mutation(workflow['jobs']['notify-marketplace']['steps'][0])
            with self.subTest(step=workflow['jobs']['notify-marketplace']['steps'][0]), self.assertRaises(AssertionError):
                validate_policy('release.yml', workflow)

    def test_release_credential_stays_in_the_single_notification_step(self):
        for mutation in (
            lambda w: w.__setitem__('env', {'TOKEN': '${{ secrets.MARKETPLACE_DISPATCH_TOKEN }}'}),
            lambda w: w['jobs']['release'].__setitem__('env', {'TOKEN': '${{ secrets.MARKETPLACE_DISPATCH_TOKEN }}'}),
            lambda w: w['jobs']['notify-marketplace']['steps'].append({'run': 'echo extra'}),
            lambda w: w['jobs'].__setitem__('extra', deepcopy(w['jobs']['notify-marketplace'])),
            lambda w: w['jobs'].pop('notify-marketplace'),
        ):
            workflow = deepcopy(self.workflows['release.yml'])
            mutation(workflow)
            with self.subTest(workflow=workflow), self.assertRaises(AssertionError):
                validate_policy('release.yml', workflow)

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

    def test_jobs_reject_unbounded_execution_and_persisted_checkout_tokens(self):
        workflow = deepcopy(self.workflows['checks.yml'])
        del workflow['jobs']['rust-and-mcp']['timeout-minutes']
        with self.assertRaises(AssertionError):
            validate_policy('checks.yml', workflow)
        workflow = deepcopy(self.workflows['checks.yml'])
        workflow['jobs']['rust-and-mcp']['steps'][0]['with']['persist-credentials'] = 'true'
        with self.assertRaises(AssertionError):
            validate_policy('checks.yml', workflow)

    def test_automated_updates_cover_all_dependency_ecosystems(self):
        config = yaml.safe_load((ROOT / '.github/dependabot.yml').read_text())
        self.assertEqual(config['version'], 2)
        self.assertEqual({u['package-ecosystem'] for u in config['updates']},
                         {'github-actions', 'cargo', 'npm', 'pip'})
        for update in config['updates']:
            self.assertEqual(update['directory'], '/')
            self.assertEqual(update['schedule']['interval'], 'weekly')
        self.assertTrue((ROOT / 'Cargo.lock').is_file())
        self.assertNotIn('Cargo.lock', (ROOT / '.gitignore').read_text().splitlines())

    def test_dependency_scans_fail_on_findings_and_preserve_evidence(self):
        workflow = self.workflows['dependencies.yml']
        steps = workflow['jobs']['dependency-audit']['steps']
        commands = '\n'.join(step.get('run', '') for step in steps)
        for command in ('--lockfile Cargo.lock', '--lockfile package-lock.json',
                        'npm audit --json', 'pip_audit -r requirements-ci.txt', 'sha256sum --check'):
            self.assertIn(command, commands)
        self.assertNotIn('continue-on-error', str(workflow))
        self.assertNotIn('|| true', commands)
        self.assertEqual(steps[-1]['with']['if-no-files-found'], 'error')

    def test_validation_categories_remain_present(self):
        commands = '\n'.join(
            step.get('run', '')
            for name, workflow in self.workflows.items() if name not in DEPLOYMENTS
            for job in workflow['jobs'].values()
            for step in job.get('steps', [])
        )
        for required in ('cargo fmt --check', 'cargo test --locked',
                         'cargo clippy --locked --target wasm32-unknown-unknown -- -D warnings',
                         'ruff check', 'unittest discover', 'test:integration',
                         'scripts/check_plugin.py', 'scripts/check_portable_schema.py',
                         'skills-ref validate', 'docs/build.py', 'docs/check.py',
                         'scripts/check_prose.py', 'actionlint'):
            with self.subTest(category=required):
                self.assertIn(required, commands)


if __name__ == '__main__':
    unittest.main()
