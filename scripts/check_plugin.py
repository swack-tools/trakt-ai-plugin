#!/usr/bin/env python3
"""Repository package contract checks, not a substitute for portal validation."""
import argparse
import json
from pathlib import Path
import re
import struct

ROOT = Path(__file__).resolve().parents[1]
SKILLS = {'what-to-watch','watching-profile','find-title','connection-help',
          'lists-and-watchlist','upcoming-releases','manage-library'}
TOOLS = {'trakt_search','trakt_get_watched_history','trakt_get_recommendations',
         'trakt_request_login','trakt_confirm_login','trakt_list_operations',
         'trakt_get_operation','trakt_api_read','trakt_api_write'}
FORBIDDEN_NAMES = {'.DS_Store','Thumbs.db','node_modules','.git','.env','wrangler.toml','__pycache__'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def inside(root, value):
    require(value.startswith('./'),f'Package path must begin ./ : {value}')
    target=(root/value).resolve()
    require(target.is_relative_to(root.resolve()) and target.exists(),f'Missing or escaping package reference: {value}')
    return target


def frontmatter(path):
    text=path.read_text()
    require(text.startswith('---\n'),f'Missing frontmatter: {path}')
    header, body=text[4:].split('\n---\n',1)
    fields={}
    for key in ('name','description'):
        match=re.search(rf'^{key}:\s*(.+)$',header,re.M)
        require(match is not None,f'Missing skill {key}: {path}')
        value=match.group(1).strip().strip('"\'')
        if value in ('>','|','>-','|-'):
            rest=header[match.end():]
            value=' '.join(line.strip() for line in rest.splitlines() if line.startswith(' '))
        require(bool(value),f'Empty skill {key}: {path}')
        fields[key]=value
    return fields,body


def validate(plugin, repository_checks=True):
    plugin=plugin.resolve()
    files=list(plugin.rglob('*'))
    require(files,'Empty package')
    total=0
    portable_names=set()
    require(len([p for p in files if p.is_file()]) <= 512, 'Claude directory plugin file limit exceeded')
    for p in files:
        require(not p.is_symlink(),f'Symlink prohibited: {p}')
        rel=p.relative_to(plugin)
        folded=rel.as_posix().casefold()
        require(folded not in portable_names, f'Case-colliding path: {rel}')
        portable_names.add(folded)
        reserved={'CON','PRN','AUX','NUL',*(f'COM{i}' for i in range(1,10)),*(f'LPT{i}' for i in range(1,10))}
        require(not any(part.split('.')[0].upper() in reserved for part in rel.parts),f'Reserved platform filename: {rel}')
        require(not any(x in FORBIDDEN_NAMES or x.startswith('._') for x in rel.parts),f'Prohibited artifact: {rel}')
        require(all(re.fullmatch(r'[A-Za-z0-9_.-]+',part) and not part.endswith(('.', ' ')) for part in rel.parts),f'Nonportable filename: {rel}')
        if not p.is_file():continue
        data=p.read_bytes();total+=len(data)
        require(len(data)<5*1024*1024,f'File exceeds Claude 5MiB limit: {rel}')
        require(not data.startswith(b'version https://git-lfs.github.com/spec/v1'),f'LFS pointer: {rel}')
        require(not any(x in data for x in (b'-----BEGIN PRIVATE KEY-----',b'-----BEGIN OPENSSH PRIVATE KEY-----',b'[TODO:')),f'Secret/placeholder marker: {rel}')
        require(not re.search(rb'(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|fc-[a-f0-9]{30,})',data),f'Credential pattern: {rel}')
        if p.suffix=='.png':
            require(data[:8]==b'\x89PNG\r\n\x1a\n',f'Invalid PNG: {rel}')
            width,height=struct.unpack('>II',data[16:24]);require(width==height and width>=128,f'Icon dimensions: {rel}')
        elif p.suffix not in ('.md','.json','.svg','.yaml','.yml') and p.name!='LICENSE':
            raise ValueError(f'Unexpected runtime artifact type: {rel}')
        else:
            data.decode('utf-8')
            require(len(data)<256*1024,f'Text file exceeds Claude 256KiB limit: {rel}')
    require(total<=20*1024*1024,'Package exceeds local 20MiB safety budget')
    manifests=[json.loads((plugin/x).read_text()) for x in ('.claude-plugin/plugin.json','.codex-plugin/plugin.json','plugin.json')]
    require({x['name'] for x in manifests}=={'trakt-mcp'},'Manifest identity mismatch')
    require(len({x['version'] for x in manifests})==1,'Manifest version mismatch')
    require(re.fullmatch(r'\d+\.\d+\.\d+',manifests[0]['version']), 'Use a release semver')
    interface=manifests[2]['extensions']['com.openai']['interface']
    require(manifests[1].get('interface')==interface,'Portable and Codex interfaces differ')
    require(set(interface.get('capabilities',[]))=={'Read','Write'},'Declare read and write capabilities')
    for m in manifests:
        require(m.get('license')=='GPL-3.0-only','Preserve license identifier')
        require(bool(m.get('description')),'Missing description')
        def paths(obj):
            if isinstance(obj,dict):
                for v in obj.values(): paths(v)
            elif isinstance(obj,list):
                for v in obj: paths(v)
            elif isinstance(obj,str) and obj.startswith('./'):inside(plugin,obj)
        paths(m)
    require((plugin/'LICENSE').read_text().startswith('                    GNU GENERAL PUBLIC LICENSE'),'Missing GPL license')
    readme=(plugin/'README.md').read_text()
    prose=re.sub(r'```.*?```','',readme,flags=re.S)
    require(len(prose.split())>=40,'Claude README requires at least40 prose words')
    configs=[json.loads((plugin/x).read_text()) for x in ('.mcp.json','mcp.json')]
    endpoints=[]
    for config in configs:
        servers=config['mcpServers'];require(len(servers)==1,'Expected one Trakt connection')
        for server in servers.values():
            require(server.get('type') in ('http','streamable-http'),'Use Streamable HTTP')
            url=server.get('url','');require(url.startswith('https://') and url.endswith('/mcp'),'Invalid production MCP URL')
            require(set(server)<= {'type','url','description'},'Unexpected connection configuration; check credentials')
            endpoints.append(url)
    require(len(set(endpoints))==1,'Platform MCP URLs differ')
    skills=plugin/'skills'
    require({p.name for p in skills.iterdir() if p.is_dir()}==SKILLS,
            f'Expected {len(SKILLS)} distinct skill workflows: {sorted(SKILLS)}')
    mentioned=set()
    for skill in sorted(skills.iterdir()):
        fm,body=frontmatter(skill/'SKILL.md')
        require(fm['name']==skill.name,'Skill folder/name mismatch')
        require(len(fm['name'])<=64 and len(fm['description'])<=1024,'Skill metadata too long')
        require(len(body.split())>=180,'Skill needs substantive workflow instructions')
        metadata=skill/'agents/openai.yaml'
        require(metadata.is_file(),f'Missing standalone skill dependency metadata: {skill.name}')
        dependency_urls=re.findall(r'^\s+url:\s*[\"\']?([^\"\'\s]+)[\"\']?\s*$',metadata.read_text(),re.M)
        require(dependency_urls==[endpoints[0]],f'Skill MCP dependency differs: {skill.name}')
        actual=set(re.findall(r'\btrakt_[a-z_]+\b',body));mentioned|=actual&TOOLS
        # Error identifiers share the trakt_ prefix and are not necessarily tool names.
        require(not any(x in body for x in ('mcp__trakt__','mcp__plugin_')),'Do not hardcode a client tool namespace')
        for target in re.findall(r'\[[^\]]*\]\(([^)]+)\)',body):
            if '://' in target or target.startswith('#'):continue
            ref=(skill/target.split('#')[0]).resolve()
            require(ref.is_relative_to(skill) and ref.is_file(),f'Standalone skill missing bundled reference: {target}')
    require(mentioned==TOOLS,'Skills do not cover actual tool contract')
    if repository_checks:
        require((ROOT/'LICENSE').read_text().strip()==(plugin/'LICENSE').read_text().strip(),'Bundled GPL license differs')
        for path in ('.claude-plugin/marketplace.json','.agents/plugins/marketplace.json'):
            marketplace=json.loads((ROOT/path).read_text())
            entry=next(x for x in marketplace['plugins'] if x['name']=='trakt-mcp')
            source=entry['source'];source=source['path'] if isinstance(source,dict) else source
            require((ROOT/source).resolve()==plugin,'Marketplace must resolve canonical plugin')
        for path in ('.mcp.json','.claude-plugin/mcp.json'):
            legacy=json.loads((ROOT/path).read_text());require(legacy==configs[0],'Root compatibility connection differs')
    print(f'Validated {len([p for p in files if p.is_file()])} files, {len(SKILLS)} skills, {len(TOOLS)} tools, manifests, paths, license, assets, and MCP configuration')


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('plugin',nargs='?',type=Path,default=ROOT/'plugins/trakt-mcp')
    validate(parser.parse_args().plugin)
