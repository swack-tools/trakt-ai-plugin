#!/usr/bin/env python3
"""Read GitHub settings and fail if the declared repository protections drift."""
import argparse
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def validate(expected, actual, path='policy'):
    """Compare required values, allowing unrelated GitHub response metadata."""
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            raise ValueError(f'{path}: expected an object')
        for key, value in expected.items():
            if key not in actual:
                raise ValueError(f'{path}.{key}: missing')
            validate(value, actual[key], f'{path}.{key}')
    elif isinstance(expected, list):
        if not isinstance(actual, list) or len(expected) != len(actual):
            raise ValueError(f'{path}: list differs')
        for item in expected:
            if item not in actual:
                raise ValueError(f'{path}: required entry missing: {item}')
    elif expected != actual or type(expected) is not type(actual):
        raise ValueError(f'{path}: expected {expected!r}, received {actual!r}')


def check_inventory(policy, inventory):
    protection = dict(policy['protection'])
    # GitHub represents these requested booleans as {enabled: boolean} objects.
    for name in ('enforce_admins', 'required_linear_history', 'allow_force_pushes',
                 'allow_deletions', 'block_creations', 'required_conversation_resolution',
                 'lock_branch', 'allow_fork_syncing'):
        protection[name] = {'enabled': protection[name]}
    actual_protection = dict(inventory['protection'])
    actual_protection.setdefault('restrictions', None)
    validate(protection, actual_protection, 'protection')
    bypasses = actual_protection['required_pull_request_reviews'].get('bypass_pull_request_allowances', {})
    if any(bypasses.values()):
        raise ValueError('protection: pull request review bypasses must be empty')
    validate(policy['workflow_permissions'], inventory['workflow_permissions'], 'workflow_permissions')
    validate({'total_count': 0}, inventory['runners'], 'runners')
    validate({'enabled': True, 'paused': False}, inventory['automated_security_fixes'], 'automated_security_fixes')
    if not inventory['vulnerability_alerts']:
        raise ValueError('vulnerability_alerts: disabled')


def github(repo, endpoint):
    result = subprocess.run(['gh', 'api', f'repos/{repo}/{endpoint}'],
                            check=True, capture_output=True, text=True)
    return json.loads(result.stdout) if result.stdout.strip() else True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, help='Save non-secret GitHub evidence as JSON')
    args = parser.parse_args()
    policy = json.loads((ROOT / '.github/repository-policy.json').read_text())
    repo = policy['repository']
    inventory = {
        'repository': repo,
        'protection': github(repo, f'branches/{policy["branch"]}/protection'),
        'workflow_permissions': github(repo, 'actions/permissions/workflow'),
        'runners': github(repo, 'actions/runners'),
        'automated_security_fixes': github(repo, 'automated-security-fixes'),
        'vulnerability_alerts': github(repo, 'vulnerability-alerts'),
    }
    if args.output:
        args.output.write_text(json.dumps(inventory, indent=2) + '\n')
    check_inventory(policy, inventory)
    print('Repository protection, required checks, permissions, runners, and security updates verified.')


if __name__ == '__main__':
    main()
