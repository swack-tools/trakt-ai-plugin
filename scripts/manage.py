#!/usr/bin/env python3
"""Local configuration/secret helper. Never prints secret values."""
import json, os, re, subprocess, sys, tempfile
from urllib.parse import urlsplit
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent

def environment():
    values = dict(os.environ)
    env_file = ROOT / '.env'
    for line in env_file.read_text().splitlines() if env_file.exists() else []:
        if line.strip() and not line.lstrip().startswith('#') and '=' in line:
            k, v = line.split('=', 1)
            values[k.strip()] = v.strip().strip('"').strip("'")
    return values

def public_base(values=None):
    values = environment() if values is None else values
    value = values.get('PUBLIC_BASE_URL') or ('https://' + values.get('CLOUDFLARE_PLUGIN_DNS', ''))
    parsed = urlsplit(value)
    if parsed.scheme != 'https' or not parsed.hostname or parsed.username or parsed.password or parsed.path not in ('', '/') or parsed.query or parsed.fragment:
        raise SystemExit('Set PUBLIC_BASE_URL to an HTTPS origin, or configure CLOUDFLARE_PLUGIN_DNS.')
    return value.rstrip('/')

def main():
    v = environment()
    if sys.argv[1] == 'configure':
        host = v.get('CLOUDFLARE_PLUGIN_DNS', '')
        zone = v.get('CLOUDFLARE_ZONE_NAME', '')
        valid_host = lambda value: len(value) <= 253 and all(re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?', label) for label in value.split('.'))
        if not valid_host(host) or not valid_host(zone) or not (host == zone or host.endswith('.' + zone)):
            raise SystemExit('Set CLOUDFLARE_PLUGIN_DNS and its explicit CLOUDFLARE_ZONE_NAME to valid hostnames.')
        v['PUBLIC_BASE_URL'] = public_base(v)
        if urlsplit(v['PUBLIC_BASE_URL']).netloc != host:
            raise SystemExit('PUBLIC_BASE_URL must match CLOUDFLARE_PLUGIN_DNS for these Worker routes.')
        if (ROOT / '.env').exists():
            os.chmod(ROOT / '.env', 0o600)
        template = (ROOT / 'wrangler.toml.example').read_text()
        for k in ('CLOUDFLARE_ACCOUNT_ID', 'CLOUDFLARE_KV_NAMESPACE', 'CLOUDFLARE_PLUGIN_DNS', 'CLOUDFLARE_ZONE_NAME', 'PUBLIC_BASE_URL'):
            if not v.get(k) or any(c in v[k] for c in '\"\\\n\r'):
                raise SystemExit('Missing or unsafe configuration value: ' + k)
            template = template.replace('${' + k + '}', v[k])
        (ROOT / 'wrangler.toml').write_text(template)
        os.chmod(ROOT / 'wrangler.toml', 0o600)
        print('Configured Worker.')
    elif sys.argv[1] == 'sync-secrets':
        mapping = {'CLOUDFLARE_ACCOUNT_ID': 'CLOUDFLARE_ACCOUNT_ID', 'CLOUDFLARE_API_TOKEN': 'CLOUDFLARE_WORKER_API_TOKEN', 'CLOUDFLARE_KV_NAMESPACE': 'CLOUDFLARE_KV_NAMESPACE', 'TRAKT_CLIENT_ID': 'TRAKT_CLIENT_ID', 'TRAKT_CLIENT_SECRET': 'TRAKT_CLIENT_SECRET'}
        for name, source in mapping.items():
            subprocess.run(['gh', 'secret', 'set', name], input=v[source], text=True, cwd=ROOT, check=True)
            print('Synced ' + name)
    elif sys.argv[1] == 'deploy':
        e = os.environ | {'CLOUDFLARE_API_TOKEN': v['CLOUDFLARE_WORKER_API_TOKEN'], 'CLOUDFLARE_ACCOUNT_ID': v['CLOUDFLARE_ACCOUNT_ID']}
        with tempfile.NamedTemporaryFile(mode='w', suffix='.json') as f:
            json.dump({k: v[k] for k in ('TRAKT_CLIENT_ID', 'TRAKT_CLIENT_SECRET', 'OPENAI_APPS_CHALLENGE', 'SUPPORT_EMAIL') if v.get(k)}, f)
            f.flush()
            subprocess.run(['npx', 'wrangler', 'deploy', '--secrets-file', f.name], cwd=ROOT, env=e, check=True)
    else:
        raise SystemExit('Usage: manage.py configure|sync-secrets|deploy')
if __name__ == '__main__':
    main()
