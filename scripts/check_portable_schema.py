#!/usr/bin/env python3
"""Validate portable manifests against pinned public schemas (development only)."""
import hashlib
import json
from pathlib import Path
import urllib.request
import jsonschema

ROOT=Path(__file__).resolve().parents[1]
SCHEMAS={
 'plugin.json':('https://agent-plugins.org/schemas/1.0.0/plugin.schema.json','0a4aad95ce337878ad38802ebf0daa3fde76abe3f65400c86bcbb1ec0b3ab883'),
 'mcp.json':('https://agent-plugins.org/schemas/1.0.0/mcp.schema.json','6539175bfcdf43085855183e86da40ea94b166547a72b47ae9a0a390516d3acb'),
}
for filename,(url,digest) in SCHEMAS.items():
 request=urllib.request.Request(url,headers={'User-Agent':'trakt-plugin-schema-validation/1.0'})
 with urllib.request.urlopen(request,timeout=30) as response: raw=response.read(128*1024)
 if hashlib.sha256(raw).hexdigest()!=digest:raise SystemExit('Schema changed; review upstream before updating pin: '+url)
 schema=json.loads(raw)
 jsonschema.Draft202012Validator(schema,format_checker=jsonschema.FormatChecker()).validate(json.loads((ROOT/'plugins/trakt-mcp'/filename).read_text()))
 print('Official Agent Plugins 1.0.0 schema passed:',filename)
