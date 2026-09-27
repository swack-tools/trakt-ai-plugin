#!/usr/bin/env python3
"""Public deployment checks; no account data is accessed."""
import json,time,urllib.request,urllib.error
from manage import public_base
BASE=public_base()
def get(path):
 try:
  with urllib.request.urlopen(urllib.request.Request(BASE+path,headers={'User-Agent':'trakt-mcp-smoke/1.0'}),timeout=20) as r:return r.status,json.load(r)
 except urllib.error.HTTPError as e:return e.code,json.load(e)
for attempt in range(12):
 try:
  status,data=get('/health')
  if status==200 and data['status']=='ok':break
 except Exception:
  if attempt==11:raise
 time.sleep(5)
else:raise SystemExit('Health check failed')
for path in ['/openapi.json','/.well-known/oauth-authorization-server','/.well-known/oauth-protected-resource']:
 status,_=get(path);assert status==200,(path,status)
assert get('/.well-known/oauth-authorization-server')[1]['issuer']==BASE
assert get('/.well-known/oauth-protected-resource')[1]['resource']==BASE+'/'
assert get('/openapi.json')[1]['servers'][0]['url']==BASE
assert get('/sync/watched')[0]==401
print('Custom-domain health, discovery and authentication checks passed.')
