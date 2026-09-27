"""Exercise portable config generation without loading developer credentials."""
import importlib.util
from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('manage', ROOT / 'scripts/manage.py')
manage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(manage)

class ConfigurationTests(unittest.TestCase):
    def test_custom_deployment_renders_without_production_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'wrangler.toml.example').write_text((ROOT / 'wrangler.toml.example').read_text())
            values = dict(CLOUDFLARE_ACCOUNT_ID='a' * 32, CLOUDFLARE_KV_NAMESPACE='b' * 32,
                          CLOUDFLARE_PLUGIN_DNS='watch.example.org', CLOUDFLARE_ZONE_NAME='example.org')
            with patch.object(manage, 'ROOT', root), patch.object(manage, 'environment', return_value=values), patch('sys.argv', ['manage.py', 'configure']):
                manage.main()
            path = root / 'wrangler.toml'
            config = tomllib.loads(path.read_text())
            self.assertEqual(config['vars']['PUBLIC_BASE_URL'], 'https://watch.example.org')
            self.assertEqual(config['kv_namespaces'][0]['binding'], 'TRAKT_SESSIONS')
            self.assertTrue(all(route['pattern'].startswith('watch.example.org/') for route in config['routes']))
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertNotIn('${', path.read_text())

    def test_rejects_unsafe_public_origins(self):
        for value in ['http://example.org', 'https://user@example.org', 'https://example.org/path',
                      'https://example.org?secret=value', 'https://example.org/#fragment']:
            with self.subTest(value=value), self.assertRaises(SystemExit):
                manage.public_base({'PUBLIC_BASE_URL': value})
        self.assertEqual(manage.public_base({'PUBLIC_BASE_URL': 'https://example.org/'}), 'https://example.org')

    def test_rejects_dns_mismatch_and_config_injection(self):
        for dns, zone in [('watch.example.org', 'unrelated.org'), ('bad"host', 'example.org'), ('', '')]:
            with patch.object(manage, 'environment', return_value={'CLOUDFLARE_PLUGIN_DNS': dns, 'CLOUDFLARE_ZONE_NAME': zone}), patch('sys.argv', ['manage.py', 'configure']), self.assertRaises(SystemExit):
                manage.main()

if __name__ == '__main__':
    unittest.main()
