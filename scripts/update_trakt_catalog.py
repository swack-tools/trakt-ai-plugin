#!/usr/bin/env python3
"""Refresh the allowlisted Trakt operations from official per-endpoint OpenAPI.

Uses Firecrawl CLI (authenticated separately), at most two concurrent requests.
Raw pages stay in ignored .firecrawl; the committed output contains request
schema facts, not copied response examples or long reference prose.
"""
from __future__ import annotations
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
CACHE = ROOT / '.firecrawl/trakt-catalog'
OUTPUT = ROOT / 'api/trakt/catalog.json'
SOURCES = ROOT / 'api/trakt/sources.json'
INDEX = 'https://docs.trakt.tv/reference/llms.txt'
REQUEST_LOCK = threading.Lock()
LAST_REQUEST = 0.0
METHODS = {'get', 'post', 'put', 'delete', 'patch', 'head', 'options'}
ESCAPES = re.compile(r'\\([!"#$%&\'()*+,\-./:;<=>?@\[\]\\^_`{|}~])')


def scrape(url: str, output: Path, refresh: bool = False) -> str:
    if refresh or not output.exists():
        output.parent.mkdir(parents=True, exist_ok=True)
        global LAST_REQUEST
        for attempt in range(5):
            with REQUEST_LOCK:
                time.sleep(max(0, 1.8 - (time.monotonic() - LAST_REQUEST)))
                LAST_REQUEST = time.monotonic()
            result = subprocess.run(['npx', '--yes', 'firecrawl-cli', 'scrape', url,
                                     '-o', str(output)], capture_output=True, text=True)
            if result.returncode == 0:
                break
            if 'Rate limit exceeded' in result.stderr and attempt < 4:
                match = re.search(r'retry after (\d+)s', result.stderr)
                time.sleep(min(60, int(match.group(1)) + 2) if match else 5)
                continue
            raise RuntimeError(f'Firecrawl failed for {url}: {result.stderr[:500]}')
    return output.read_text()


def parse_document(text: str) -> dict:
    # Firecrawl Markdown-escapes literal Markdown served as text/plain.
    if r'\# OpenAPI definition' in text:
        text = ESCAPES.sub(r'\1', text).replace('\\\n', '\n')
    marker = text.index('OpenAPI definition')
    start = text.index('{', marker)
    document, _ = json.JSONDecoder().raw_decode(text[start:])
    if not document.get('paths'):
        raise ValueError('No OpenAPI paths in reference page')
    return document


def resolve(value, document, stack=()):
    if isinstance(value, list):
        return [resolve(v, document, stack) for v in value]
    if not isinstance(value, dict):
        return value
    if '$ref' in value:
        ref = value['$ref']
        if not ref.startswith('#/') or ref in stack:
            raise ValueError(f'Unsupported external or recursive schema reference: {ref}')
        target = document
        for part in ref[2:].split('/'):
            target = target[part.replace('~1', '/').replace('~0', '~')]
        return resolve({**target, **{k:v for k,v in value.items() if k != '$ref'}}, document, (*stack, ref))
    return {k:resolve(v, document, stack) for k,v in value.items()}


def json_schema(schema):
    """Translate OpenAPI 3.0 nullable into JSON Schema, retaining constraints."""
    if isinstance(schema, list):
        return [json_schema(v) for v in schema]
    if not isinstance(schema, dict):
        return schema
    # Never strip a property named description, title, or examples.
    result = {}
    for key, value in schema.items():
        if key in {'nullable', 'description', 'example', 'examples', 'externalDocs', 'xml', 'title'}:
            continue
        if key in {'properties', 'patternProperties', '$defs', 'definitions'}:
            result[key] = {name:json_schema(child) for name,child in value.items()}
        else:
            result[key] = json_schema(value)
    if 'properties' in result and 'additionalProperties' not in result:
        result['additionalProperties'] = False
    if schema.get('nullable'):
        return {'anyOf': [result, {'type':'null'}]}
    return result


def extract(url: str, refresh: bool = False) -> list[dict]:
    slug = url.rsplit('/', 1)[-1]
    text = scrape(url, CACHE / slug, refresh)
    document = parse_document(text)
    operations = []
    for path, item in document['paths'].items():
        for method, operation in item.items():
            if method not in METHODS:
                continue
            desc = operation.get('description', '')
            auth = 'required' if 'OAuth Required' in desc else 'optional' if 'OAuth Optional' in desc else 'none'
            params = []
            for p in item.get('parameters', []) + operation.get('parameters', []):
                p = resolve(p, document)
                if p['in'] not in {'path', 'query'}:
                    continue
                params.append({'name':p['name'], 'in':p['in'],
                               'required':p.get('required', False),
                               'schema':json_schema(p.get('schema', {})),
                               **{key:p[key] for key in ('style', 'explode', 'allowReserved') if key in p}})
            paginated = 'Pagination' in desc or any(p['name'] == 'page' for p in params)
            if paginated:
                for name, maximum in [('page', 4294967295), ('limit', 100)]:
                    parameter = next((p for p in params if p['in'] == 'query' and p['name'] == name), None)
                    if parameter is None:
                        parameter = {'name':name, 'in':'query', 'required':False}
                        params.append(parameter)
                    parameter['schema'] = {'type':'integer', 'minimum':1, 'maximum':maximum}
            body = operation.get('requestBody')
            request_body = None
            if body:
                body = resolve(body, document)
                content = body.get('content', {})
                media = content.get('application/json')
                if media is not None:
                    request_body = {'required':body.get('required', bool(media.get('schema', {}).get('required'))),
                                    'schema':json_schema(media.get('schema', {}))}
            status = 'supported'
            reason = None
            if path.startswith('/oauth/'):
                status = 'managed_auth'
                reason = 'Handled by the managed OAuth connection; never exposed as a raw credential tool.'
            elif re.search(r'only to first-party|first.party (?:Trakt )?(?:applications|apps) only|third-party applications receive', desc, re.I):
                status = 'unavailable'
                reason = 'Official reference limits this operation to first-party Trakt applications.'
            elif body and request_body is None:
                status = 'unavailable'
                reason = 'The documented request uses a non-JSON content type.'
            restrictions = []
            if 'VIP' in desc:
                restrictions.append('Account VIP features or limits apply; see source documentation.')
            if 'OAuth' not in desc and '/calendars/{target}' in path:
                auth = 'optional'
                restrictions.append('The my target requires OAuth; the all target is global.')
            if 'Limited Access' in desc:
                restrictions.append('The official reference marks this operation as limited access.')
            operation_id = operation['operationId']
            record = {'operation_id': operation_id, 'method':method.upper(), 'path':path,
                      'source_url':url, 'summary':operation.get('summary', operation_id),
                      'description':f"{method.upper()} {path}",
                      'categories':operation.get('tags', []), 'parameters':params,
                      'request_body':request_body, 'auth':auth,
                      'pagination':{'supported':paginated},
                      'status':status}
            if request_body is not None:
                record['schema_normalization_note'] = 'Objects with documented properties reject unknown fields unless the source explicitly allows additional properties. Use the documented schema and supply a single supported identifier in oneOf identifier objects.'
            if paginated:
                record['normalization_note'] = 'Pagination page and limit use positive integer inputs; MCP locally caps limit at 100 and page at 4294967295. Pagination parameters are included for documented paginated operations.'
            if reason:
                record['status_reason'] = reason
            if restrictions:
                record['restrictions'] = restrictions
            operations.append(record)
    return operations


def offline_check():
    catalog = json.loads(OUTPUT.read_text())
    sources = json.loads(SOURCES.read_text())
    urls = sources['operation_urls']
    operations = catalog['operations']
    identities = [operation['operation_id'] for operation in operations]
    operation_urls = [operation['source_url'] for operation in operations]
    expected = len(urls)
    checks = {
        'complete catalog': not catalog.get('incomplete', False),
        'matching operation count': catalog['operation_count'] == expected == len(operations),
        'matching page count': catalog['source_page_count'] == expected,
        'unique operation IDs': len(set(identities)) == expected,
        'unique source URLs': len(set(urls)) == expected == len(set(operation_urls)),
        'complete source coverage': sorted(operation_urls) == urls == sorted(urls),
        'official source index': sources['source_index'] == catalog['source_index'] == INDEX,
        'inventory digest': catalog['source_index_sha256'] == hashlib.sha256('\n'.join(urls).encode()).hexdigest(),
        'resolved schemas': '"$ref"' not in json.dumps(operations),
        'availability reasons': all(o['status'] == 'supported' or o.get('status_reason') for o in operations),
        'known availability statuses': all(o['status'] in {'supported', 'managed_auth', 'unavailable'} for o in operations),
        'unique method paths': len({(o['method'],o['path']) for o in operations}) == expected,
        'matched path parameters': all(set(re.findall(r'{([^}]+)}', o['path'])) == {p['name'] for p in o['parameters'] if p['in'] == 'path'} for o in operations),
    }
    failed = [name for name, passed in checks.items() if not passed]
    if failed:
        raise SystemExit('Catalog integrity failed: ' + ', '.join(failed))
    print(f'Offline integrity verified: {expected} operations and official source URLs')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--offline-check', action='store_true', help='Validate committed catalog and source inventory without network or credentials')
    parser.add_argument('--refresh', action='store_true', help='Refetch every source page')
    parser.add_argument('--check', action='store_true', help='Check generated file without writing')
    parser.add_argument('--workers', type=int, choices=(1,2), default=2)
    args = parser.parse_args()
    if args.offline_check:
        offline_check()
        return
    text = scrape(INDEX, CACHE / 'index.md', args.refresh)
    urls = sorted(set(re.findall(r'https://docs\.trakt\.tv/reference/(?:get|post|put|delete|patch|head|options)[^\s)]+', text)))
    if not urls:
        raise SystemExit('No endpoint pages found in official reference index')
    operations = []
    errors = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = {pool.submit(extract,url,args.refresh):url for url in urls}
        for count, future in enumerate(as_completed(futures), 1):
            try:
                operations.extend(future.result())
            except Exception as error:
                errors.append((futures[future], str(error)))
                print(f'ERROR {futures[future]}: {error}', flush=True)
            if count % 10 == 0 or count == len(urls):
                print(f'Processed {count}/{len(urls)} pages; {len(operations)} operations; {len(errors)} errors', flush=True)
    if errors:
        for url, error in errors:
            print(f'{url}: {error}', file=sys.stderr)
        raise SystemExit('Incomplete inventory; existing catalog was not overwritten')
    operations.sort(key=lambda item:item['operation_id'])
    identities = [item['operation_id'] for item in operations]
    if len(set(identities)) != len(identities):
        raise SystemExit('Duplicate operation IDs in official reference')
    catalog = {'schema_version':1, 'source_index':INDEX,
               'source_index_sha256':hashlib.sha256('\n'.join(urls).encode()).hexdigest(),
               'source_page_count':len(urls), 'operation_count':len(operations),
               'operations':operations}
    source_inventory = json.dumps({'source_index':INDEX, 'operation_urls':urls}, indent=2) + '\n'
    rendered = json.dumps(catalog, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != rendered or not SOURCES.exists() or SOURCES.read_text() != source_inventory:
            raise SystemExit('Catalog differs; run scripts/update_trakt_catalog.py')
    else:
        OUTPUT.parent.mkdir(parents=True, exist_ok=True)
        temporary = OUTPUT.with_suffix('.json.tmp')
        temporary.write_text(rendered)
        temporary.replace(OUTPUT)
        SOURCES.write_text(source_inventory)
    print(f'Validated {len(operations)} operations in {OUTPUT.relative_to(ROOT)}')


if __name__ == '__main__':
    main()
