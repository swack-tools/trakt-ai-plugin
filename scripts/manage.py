#!/usr/bin/env python3
"""Local configuration/secret helper. Never prints secret values."""
import json, os, secrets, subprocess, sys, tempfile
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent

def environment():
    values = {}
    for line in (ROOT / '.env').read_text().splitlines():
        if line.strip() and not line.lstrip().startswith('#') and '=' in line:
            k, v = line.split('=', 1)
            values[k.strip()] = v.strip().strip('"').strip("'")
    return values

def main():
    v = environment()
    if sys.argv[1] == 'configure':
        assert v['CLOUDFLARE_PLUGIN_DNS'] == 'trakt.swacktech.com', 'Unexpected target domain'
        assert v['CLOUDFLARE_KV_NAMESPACE_NAME'] == 'TRAKT_SESSIONS', 'Unexpected KV binding'
        os.chmod(ROOT / '.env', 0o600)
        template = (ROOT / 'wrangler.toml.example').read_text()
        for k in ('CLOUDFLARE_ACCOUNT_ID', 'CLOUDFLARE_KV_NAMESPACE'):
            template = template.replace('${' + k + '}', v[k])
        (ROOT / 'wrangler.toml').write_text(template)
        print('Configured Worker.')
    elif sys.argv[1] == 'sync-secrets':
        mapping = {'CLOUDFLARE_ACCOUNT_ID': 'CLOUDFLARE_ACCOUNT_ID', 'CLOUDFLARE_API_TOKEN': 'CLOUDFLARE_WORKER_API_TOKEN', 'TRAKT_CLIENT_ID': 'TRAKT_CLIENT_ID', 'TRAKT_CLIENT_SECRET': 'TRAKT_CLIENT_SECRET'}
        for name, source in mapping.items():
            subprocess.run(['gh', 'secret', 'set', name, '--repo', 'swack-tools/trakt-mcp'], input=v[source], text=True, check=True)
            print('Synced ' + name)
    elif sys.argv[1] == 'deploy':
        e = os.environ | {'CLOUDFLARE_API_TOKEN': v['CLOUDFLARE_WORKER_API_TOKEN'], 'CLOUDFLARE_ACCOUNT_ID': v['CLOUDFLARE_ACCOUNT_ID']}
        with tempfile.NamedTemporaryFile(mode='w', suffix='.json') as f:
            json.dump({k: v[k] for k in ('TRAKT_CLIENT_ID', 'TRAKT_CLIENT_SECRET')}, f)
            f.flush()
            subprocess.run(['npx', 'wrangler', 'deploy', '--secrets-file', f.name], cwd=ROOT, env=e, check=True)
    else:
        raise SystemExit('Usage: manage.py configure|sync-secrets|deploy')
if __name__ == '__main__':
    main()
