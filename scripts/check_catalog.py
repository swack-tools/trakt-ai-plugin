#!/usr/bin/env python3
"""Validate reviewed catalog metadata offline, without running plugin code."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import sys

from bs4 import BeautifulSoup
import jsonschema
from markdown_it import MarkdownIt
from soupsieve import SelectorSyntaxError
import yaml

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = 'plugins/trakt-mcp'
SCHEMA = 'schemas/upstream-info.schema.json'
SCHEMA_SHA256 = 'd115d44f2d32c653f97c08e889587a04353c871db55821d11e126d2e14707e13'
RECEIPT = 'catalog-sources.lock.json'


class CatalogError(ValueError):
    """A safe diagnostic containing no source values or configuration contents."""


def require(condition, message):
    if not condition:
        raise CatalogError(message)


def source_path(root, value):
    require(isinstance(value, str) and bool(value), 'Source path must be a relative file path')
    parts = PurePosixPath(value).parts
    require(not value.startswith('/') and not re.search(r'[:\\\x00-\x1f]', value)
            and '..' not in parts and PurePosixPath(value).as_posix() == value,
            'Source path is not repository-relative')
    path = (root / value).resolve()
    require(path.is_relative_to(root.resolve()) and path.is_file(),
            'Source path is missing, outside the repository, or not a file')
    return path


def read_json(path):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'JSON contains a duplicate key')
            result[key] = value
        return result
    def invalid_constant(_):
        raise CatalogError('JSON contains a non-finite number')
    try:
        return json.loads(path.read_text(), object_pairs_hook=unique, parse_constant=invalid_constant)
    except (OSError, UnicodeError, json.JSONDecodeError):
        raise CatalogError('Unable to read valid UTF-8 JSON') from None


def nonempty(value):
    return isinstance(value, str) and bool(value.strip())


def resolve_source(root, source, *, allow_file=False):
    require(isinstance(source, dict), 'Source selector must be an object')
    require(set(source) <= {'path', 'format', 'mode', 'selector', 'heading_path'},
            'Unsupported source selector field')
    path = source_path(root, source.get('path'))
    text = path.read_text(encoding='utf-8')
    if allow_file and set(source) == {'path'}:
        require(bool(text.strip()), 'Source file has no evidence')
        return source['path']
    mode = source.get('mode')
    require(mode in ('section', 'lead'), 'Unsupported source selection mode')
    if source.get('format') == 'html':
        require(mode == 'section' and nonempty(source.get('selector'))
                and 'heading_path' not in source, 'HTML evidence requires a CSS selector')
        try:
            matches = BeautifulSoup(text, 'html.parser').select(source['selector'])
        except (SelectorSyntaxError, NotImplementedError):
            raise CatalogError('Invalid HTML selector') from None
        require(len(matches) == 1, 'HTML selector is missing or ambiguous')
        require(bool(matches[0].get_text(strip=True)), 'HTML selector has no text evidence')
    elif source.get('format') == 'markdown':
        require('selector' not in source, 'Markdown evidence requires a heading path')
        tokens = MarkdownIt('commonmark').enable('table').parse(text)
        headings, stack = [], []
        for i, token in enumerate(tokens):
            if token.type != 'heading_open':
                continue
            level = int(token.tag[1])
            while stack and stack[-1][0] >= level:
                stack.pop()
            stack.append((level, tokens[i + 1].content))
            headings.append((token.map[0], token.map[1], level, [title for _, title in stack]))
        lines = text.splitlines()
        if mode == 'lead':
            require('heading_path' not in source, 'Lead selection cannot specify headings')
            start = headings[0][1] if headings else 0
            end = headings[1][0] if len(headings) > 1 else len(lines)
        else:
            wanted = source.get('heading_path')
            require(isinstance(wanted, list) and wanted and all(nonempty(x) for x in wanted),
                    'Markdown evidence requires a nonempty heading path')
            matches = [h for h in headings if h[3][-len(wanted):] == wanted]
            require(len(matches) == 1, 'Markdown heading path is missing or ambiguous')
            selected = matches[0]
            start = selected[1]
            end = next((h[0] for h in headings if h[0] > selected[0] and h[2] <= selected[2]), len(lines))
        section = '\n'.join(lines[start:end])
        rendered = MarkdownIt('commonmark').enable('table').render(section)
        require(bool(BeautifulSoup(rendered, 'html.parser').get_text(strip=True)),
                'Markdown section has no text evidence')
    else:
        raise CatalogError('Unsupported source format')
    return source['path']


def literal_tools(text):
    """Accept only the reviewed function's literal helper-call return array."""
    tokens = re.findall(r'"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z0-9_]*|[^\s]', text)
    signature = ['pub', 'fn', 'tools', '(', ')', '-', '>', 'Value', '{']
    starts = [i for i in range(len(tokens)) if tokens[i:i + len(signature)] == signature]
    require(len(starts) == 1, 'Tool function signature requires adapter review')

    def closing(sequence, start):
        pairs = {'(': ')', '[': ']', '{': '}'}
        stack = []
        for i in range(start, len(sequence)):
            token = sequence[i]
            if token in pairs:
                stack.append(pairs[token])
            elif token in pairs.values():
                require(stack and stack.pop() == token, 'Unbalanced tool declaration')
                if not stack:
                    return i
        raise CatalogError('Unclosed tool declaration')

    opening = starts[0] + len(signature) - 1
    body = tokens[opening + 1:closing(tokens, opening)]
    require('return' not in body and '?' not in body, 'Alternate tool return requires adapter review')
    # Skip complete statements, including their nested delimiters, to locate
    # the tail expression. Do not search for an array anywhere in the function.
    tail, i = 0, 0
    while i < len(body):
        if body[i] in ('(', '[', '{'):
            i = closing(body, i)
        elif body[i] == ';':
            tail = i + 1
        i += 1
    expression = body[tail:]
    require(expression[:7] == ['json', '!', '(', '{', '"tools"', ':', '[']
            and expression[-3:] == [']', '}', ')'],
            'Tool return must be a literal tools array; review the adapter')
    entries = expression[7:-3]
    names, i = [], 0
    while i < len(entries):
        require(entries[i:i + 2] == ['tool', '('], 'Non-helper tool entry requires adapter review')
        end = closing(entries, i + 1)
        args = entries[i + 2:end]
        require(len(args) >= 6 and all(args[n] == ',' for n in (1, 3, 5))
                and all(re.fullmatch(r'"(?:[^"\\]|\\.)*"', args[n]) for n in (0, 2, 4)),
                'Tool declaration adapter requires literal name, title, and description')
        names.append(json.loads(args[0]))
        i = end + 1
        if i < len(entries):
            require(entries[i] == ',', 'Tool array construction requires adapter review')
            i += 1
    require(names, 'Native tool array is empty')
    return names


def native_inventory(root):
    """Read canonical defaults only; fail closed when manifest routing changes."""
    capabilities, hook_targets = set(), set()
    manifests = [read_json(source_path(root, f'{PLUGIN}/{name}')) for name in
                 ('plugin.json', '.claude-plugin/plugin.json', '.codex-plugin/plugin.json')]
    require(all(m.get('name') == 'trakt-mcp' for m in manifests), 'Native plugin identity mismatch')
    require(len({m.get('version') for m in manifests}) == 1, 'Native plugin versions disagree')
    settings = [manifests[0].get('extensions', {}).get('com.openai', {}), *manifests[1:]]
    defaults = {'skills': './skills', 'commands': './commands',
                'hooks': './hooks/hooks.json', 'mcpServers': './.mcp.json'}
    for config in settings:
        for kind, default in defaults.items():
            if kind in config:
                require(isinstance(config[kind], str) and config[kind].rstrip('/') == default,
                        'Native component routing changed; review the inventory adapter')
    for kind, pattern in [('skill', 'skills/*/SKILL.md'), ('command', 'commands/**/*.md')]:
        for path in sorted((root / PLUGIN).glob(pattern)):
            path = source_path(root, path.relative_to(root).as_posix())
            text = path.read_text()
            require(text.startswith('---\n'), 'Native component lacks frontmatter')
            parts = text.split('---', 2)
            require(len(parts) == 3, 'Native component lacks frontmatter')
            try:
                front = yaml.safe_load(parts[1])
            except yaml.YAMLError:
                raise CatalogError('Invalid native frontmatter') from None
            require(isinstance(front, dict) and nonempty(front.get('description')),
                    'Native component lacks a description')
            name = front.get('name', path.stem if kind == 'command' else None)
            require(isinstance(name, str) and re.fullmatch(r'[a-z0-9][a-z0-9-]*', name),
                    'Invalid native component name')
            ref = f'{kind}:{name}'
            require(ref not in capabilities, 'Duplicate native component')
            capabilities.add(ref)
    servers = []
    for name in ('mcp.json', '.mcp.json'):
        config = read_json(source_path(root, f'{PLUGIN}/{name}')).get('mcpServers')
        require(isinstance(config, dict) and config, 'Invalid native MCP declaration')
        servers.append({key: {**value, 'type': 'http' if value.get('type') == 'streamable-http'
                             else value.get('type')} for key, value in config.items()})
    require(servers[0] == servers[1], 'Native client MCP declarations disagree')
    capabilities.update(f'mcp_server:{name}' for name in servers[0])
    hook_path = f'{PLUGIN}/hooks/hooks.json'
    if (root / hook_path).exists():
        hooks = read_json(source_path(root, hook_path)).get('hooks')
        require(isinstance(hooks, dict), 'Invalid native hook declaration')
        for event, groups in hooks.items():
            require(isinstance(groups, list), 'Invalid native hook groups')
            for i, group in enumerate(groups):
                require(isinstance(group, dict) and isinstance(group.get('hooks'), list),
                        'Invalid native hook actions')
                for j, action in enumerate(group['hooks']):
                    require(isinstance(action, dict) and nonempty(action.get('type')), 'Invalid hook action')
                    capabilities.add(f'hook:{event}:{i}:{j}')
                    hook_targets.add((hook_path, f'/hooks/{event}/{i}/hooks/{j}'))
    # Match the marketplace's literal Rust tool declaration adapter. Strip comments
    # while preserving strings, so commented-out declarations never create tools.
    text = source_path(root, 'src/mcp/protocol.rs').read_text()
    lexeme = r'"(?:[^"\\]|\\.)*"|//[^\n]*|/\*.*?\*/'
    def without_comment(match):
        value = match.group()
        if value.startswith(('//', '/*')):
            require('/*' not in value[2:], 'Nested Rust comment requires adapter review')
            return ' '
        return value
    text = re.sub(lexeme, without_comment, text, flags=re.S)
    tools = literal_tools(text)
    require(len(tools) == len(set(tools)), 'Duplicate native tool name')
    require('trakt' in servers[0], 'Native tools require their declared Trakt server')
    capabilities.update(f'mcp_tool:{name}' for name in tools)
    return capabilities, hook_targets


def changelog_files(root):
    # Search every project directory, pruning conventional dependency, cache,
    # and build trees. Never follow directory symlinks outside the snapshot.
    excluded = {'.git', 'node_modules', 'target', 'dist', 'build', 'worker',
                '.wrangler', '.vale', '.ruff_cache', '.venv', 'venv', '__pycache__', '.firecrawl'}
    non_document_extensions = {
        '.rs', '.py', '.pyc', '.js', '.mjs', '.cjs', '.ts', '.tsx', '.jsx',
        '.c', '.cpp', '.h', '.hpp', '.cs', '.java', '.go', '.rb', '.php',
        '.sh', '.bash', '.zsh', '.sql', '.json', '.jsonc', '.yaml', '.yml',
        '.toml', '.lock', '.xml', '.csv', '.tsv', '.wasm', '.png', '.jpg',
        '.jpeg', '.gif', '.webp', '.svg', '.ico', '.pdf', '.zip', '.gz', '.bin'}
    candidates = []
    for directory, subdirs, files in os.walk(root, followlinks=False):
        subdirs[:] = [name for name in subdirs if name not in excluded]
        candidates.extend(Path(directory) / name for name in files)
    return {p.relative_to(root).as_posix() for p in candidates
            if p.is_file() and p.suffix.lower() not in non_document_extensions
            and re.search(r'(?:^|[-_. ])(?:changelog|changes|history)(?:$|[-_. ])', p.name, re.I)}


def reviewed_files(root, selected):
    """Conservative review scope: additions and removals change this mapping too."""
    paths = set(selected) | changelog_files(root) | {
        'catalog-info.json', SCHEMA, 'Cargo.toml', 'package.json', 'requirements-catalog.txt',
        'api/trakt/catalog.json', 'openapi.json', 'scripts/check_catalog.py',
        'Cargo.lock', 'package-lock.json'}
    for pattern in ('README*', '*CHANGELOG*', '*CHANGES*', '*HISTORY*',
                    f'{PLUGIN}/**/*', 'docs/pages/**/*', 'src/**/*.rs',
                    '.claude-plugin/*.json', '.agents/plugins/*.json', '.mcp.json'):
        for path in root.glob(pattern):
            if path.is_file():
                paths.add(path.relative_to(root).as_posix())
    return {path: hashlib.sha256(source_path(root, path).read_bytes()).hexdigest()
            for path in sorted(paths)}


def validate(root=ROOT, *, check_review=True, refresh_review=False):
    root = root.resolve()
    schema_path = source_path(root, SCHEMA)
    require(hashlib.sha256(schema_path.read_bytes()).hexdigest() == SCHEMA_SHA256,
            'Vendored schema digest mismatch; review the approved schema pin')
    schema = read_json(schema_path)
    jsonschema.Draft202012Validator.check_schema(schema)
    data = read_json(source_path(root, 'catalog-info.json'))
    require(not next(jsonschema.Draft202012Validator(schema).iter_errors(data), None),
            'Metadata does not match the approved marketplace schema')
    require(type(data['schemaVersion']) is int and data['schemaVersion'] == 1,
            'Unsupported metadata schema version')
    require(data['pluginId'] == 'trakt-mcp', 'Metadata plugin identity mismatch')
    capabilities, hook_targets = native_inventory(root)
    selected = set()
    def resolve(source, allow_file=False):
        selected.add(resolve_source(root, source, allow_file=allow_file))
    def evidence(value, allow_file=False):
        require(isinstance(value, list) and value, 'Missing source evidence')
        for source in value:
            resolve(source, allow_file)
    require('overview' in data, 'Missing catalog overview')
    resolve(data['overview'])
    examples = data.get('examples')
    require(isinstance(examples, list) and examples, 'Missing catalog examples')
    seen, covered = set(), set()
    for example in examples:
        require('source' not in example, 'Use the sources array instead of the legacy source alias')
        require(all(nonempty(example.get(field)) for field in
                    ('id', 'title', 'platform', 'input_or_trigger', 'expected_behavior')),
                'Missing required example text field')
        require(example['id'] not in seen, 'Duplicate example ID')
        seen.add(example['id'])
        prerequisites = example.get('prerequisites')
        require(isinstance(prerequisites, list) and prerequisites and all(nonempty(x) for x in prerequisites),
                'Example requires explicit prerequisites')
        refs = example.get('capability_refs')
        require(isinstance(refs, list) and refs and all(nonempty(x) for x in refs),
                'Example requires native capability references')
        require(set(refs) <= capabilities, 'Example references an unknown native capability')
        covered.update(refs)
        require(example.get('evidence') in ('documented', 'reviewed_illustration'),
                'Example requires an explicit evidence classification')
        evidence(example.get('sources'), allow_file=True)
        if example['evidence'] == 'reviewed_illustration' or 'source_digests' in example:
            digests = example.get('source_digests', {})
            require(isinstance(digests, dict), 'Illustration requires source digests')
            require(set(digests) == {source['path'] for source in example['sources']},
                    'Source digest keys must exactly match example sources')
            for source in example['sources']:
                actual = hashlib.sha256(source_path(root, source['path']).read_bytes()).hexdigest()
                require(digests.get(source['path']) == actual, 'Illustration evidence changed; review its digest')
    require(covered == capabilities, 'A native capability lacks a reviewed example')
    for platform in data.get('platforms', {}).values():
        if platform['status'] != 'not_documented':
            evidence(platform.get('sources'))
        else:
            for source in platform.get('sources', []):
                resolve(source)
    for name, note in data.get('mcpServers', {}).items():
        require(f'mcp_server:{name}' in capabilities, 'Metadata names an unknown MCP server')
        require(isinstance(note, dict) and nonempty(note.get('description')), 'Missing MCP server description')
        evidence(note.get('sources'))
    hooks = data.get('hooks', [])
    require(isinstance(hooks, list), 'Hook notes must be a list of explicit targets')
    noted_hooks = set()
    for note in hooks:
        require(isinstance(note, dict) and isinstance(note.get('target'), dict), 'Missing hook note target')
        target = note['target']
        identity = (target.get('path'), target.get('pointer'))
        require(identity in hook_targets, 'Unknown native hook action')
        require(identity not in noted_hooks, 'Duplicate native hook note')
        noted_hooks.add(identity)
        require(nonempty(note.get('description')), 'Missing hook description')
        evidence(note.get('sources'))
    require(noted_hooks == hook_targets, 'A native hook lacks a targeted catalog note')
    require('changelog' in data, 'Metadata must declare a changelog selector or explicit null')
    if data['changelog'] is not None:
        resolve(data['changelog'])
    else:
        require(not changelog_files(root), 'A changelog exists; review the null changelog selector')
    # The marketplace schema permits extension fields inside examples and notes.
    # Validate their evidence too; no nested path may bypass containment checks.
    def extra_sources(value):
        if isinstance(value, dict):
            if 'path' in value:
                if set(value) == {'path', 'pointer'}:
                    require((value['path'], value['pointer']) in hook_targets, 'Unknown native hook target')
                    source_path(root, value['path'])
                    selected.add(value['path'])
                else:
                    resolve(value, allow_file=True)
            for child in value.values():
                extra_sources(child)
        elif isinstance(value, list):
            for child in value:
                extra_sources(child)
    extra_sources(data)
    current = reviewed_files(root, selected)
    if refresh_review:
        # Only fingerprints change. Curated text and metadata are never rewritten.
        destination = root / RECEIPT
        require(not destination.is_symlink(), 'Review receipt must not be a symlink')
        destination.write_text(json.dumps(current, indent=2, sort_keys=True) + '\n')
    elif check_review:
        previous = read_json(source_path(root, RECEIPT))
        require(previous == current,
                'Catalog sources changed; review metadata, then run --refresh-reviewed-sources')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT, help='Repository snapshot to validate')
    parser.add_argument('--refresh-reviewed-sources', action='store_true',
                        help='After human review, update source fingerprints (never use in CI)')
    args = parser.parse_args()
    try:
        validate(args.root, refresh_review=args.refresh_reviewed_sources)
    except CatalogError as error:
        print(f'Catalog validation failed: {error}', file=sys.stderr)
        return 1
    except (OSError, UnicodeError, ValueError, TypeError, KeyError, AttributeError, jsonschema.SchemaError):
        # Parser errors can include source excerpts or absolute filenames.
        print('Catalog validation failed: invalid source structure or unreadable input', file=sys.stderr)
        return 1
    print('Catalog metadata, native references, selectors, and reviewed sources validated')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
