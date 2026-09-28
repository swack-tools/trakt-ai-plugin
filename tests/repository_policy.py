"""Reject weakening of live GitHub settings without requiring CI credentials."""
from copy import deepcopy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('repository_policy', ROOT / 'scripts/check_repository_policy.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RepositoryPolicyTests(unittest.TestCase):
    def setUp(self):
        self.policy = json.loads((ROOT / '.github/repository-policy.json').read_text())
        protection = deepcopy(self.policy['protection'])
        for key, value in list(protection.items()):
            if isinstance(value, bool):
                protection[key] = {'enabled': value}
        protection.pop('restrictions')  # GitHub omits disabled push restrictions.
        self.inventory = {
            'protection': protection,
            'workflow_permissions': deepcopy(self.policy['workflow_permissions']),
            'runners': {'total_count': 0, 'runners': []},
            'automated_security_fixes': {'enabled': True, 'paused': False},
            'vulnerability_alerts': True,
        }

    def test_expected_settings_and_github_metadata_pass(self):
        self.inventory['protection']['url'] = 'https://api.github.com/example'
        MODULE.check_inventory(self.policy, self.inventory)

    def test_required_reviews_admin_enforcement_and_force_pushes_cannot_be_weakened(self):
        cases = [('enforce_admins', {'enabled': False}),
                 ('allow_force_pushes', {'enabled': True}),
                 ('allow_deletions', {'enabled': True}),
                 ('required_conversation_resolution', {'enabled': False})]
        for key, value in cases:
            inventory = deepcopy(self.inventory)
            inventory['protection'][key] = value
            with self.subTest(setting=key), self.assertRaises(ValueError):
                MODULE.check_inventory(self.policy, inventory)
        for key, value in [('required_approving_review_count', 0),
                           ('dismiss_stale_reviews', False), ('require_last_push_approval', False),
                           ('bypass_pull_request_allowances', {'users': ['owner']})]:
            inventory = deepcopy(self.inventory)
            inventory['protection']['required_pull_request_reviews'][key] = value
            with self.subTest(review_setting=key), self.assertRaises(ValueError):
                MODULE.check_inventory(self.policy, inventory)

    def test_status_checks_must_be_current_complete_and_github_actions_owned(self):
        for change in ('stale', 'missing', 'wrong_app'):
            inventory = deepcopy(self.inventory)
            checks = inventory['protection']['required_status_checks']
            if change == 'stale':
                checks['strict'] = False
            elif change == 'missing':
                checks['checks'].pop()
            else:
                checks['checks'][0]['app_id'] = -1
            with self.subTest(change=change), self.assertRaises(ValueError):
                MODULE.check_inventory(self.policy, inventory)

    def test_dependency_alerts_runners_and_token_permissions_cannot_drift(self):
        for key, value in [('vulnerability_alerts', False),
                           ('automated_security_fixes', {'enabled': False, 'paused': False}),
                           ('automated_security_fixes', {'enabled': True, 'paused': True}),
                           ('runners', {'total_count': 1}),
                           ('workflow_permissions', {'default_workflow_permissions': 'write',
                                                     'can_approve_pull_request_reviews': True})]:
            inventory = deepcopy(self.inventory)
            inventory[key] = value
            with self.subTest(setting=key), self.assertRaises(ValueError):
                MODULE.check_inventory(self.policy, inventory)

    def test_required_checks_match_real_pr_job_names(self):
        import yaml
        actual = set()
        for path in (ROOT / '.github/workflows').glob('*.yml'):
            workflow = yaml.load(path.read_text(), Loader=yaml.BaseLoader)
            if 'pull_request' not in workflow['on']:
                continue
            for job_id, job in workflow['jobs'].items():
                languages = job.get('strategy', {}).get('matrix', {}).get('language')
                if languages:
                    actual.update(f'{job_id} ({language})' for language in languages)
                else:
                    actual.add(job_id)
        expected = {check['context'] for check in self.policy['protection']['required_status_checks']['checks']}
        self.assertEqual(expected, actual)


if __name__ == '__main__':
    unittest.main()
