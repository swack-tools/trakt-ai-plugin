#!/usr/bin/env python3
"""Probe an extracted archive with installed clients, without personal plugin installs.

Claude uses a temporary CLAUDE_CONFIG_DIR. Codex uses read-only plugin/read;
this proves package discovery, not installation or authenticated workflows.
"""
import argparse
import json
import os
from pathlib import Path
import select
import shutil
import subprocess
import tempfile
import time
import zipfile

ROOT=Path(__file__).resolve().parents[1]


def codex_probe(market, cwd):
    process=subprocess.Popen(['codex','app-server','--stdio'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,text=True,bufsize=1,cwd=cwd)
    def request(ident,method,params):
        process.stdin.write(json.dumps({'id':ident,'method':method,'params':params})+'\n');process.stdin.flush()
        end=time.monotonic()+45
        while time.monotonic()<end:
            ready,_,_=select.select([process.stdout],[],[],1)
            if ready:
                line=process.stdout.readline()
                if not line:raise RuntimeError('Codex app-server exited')
                item=json.loads(line)
                if item.get('id')==ident:
                    if 'error' in item:raise RuntimeError(item['error'])
                    return item['result']
        raise TimeoutError(method)
    try:
        request(1,'initialize',{'clientInfo':{'name':'trakt-package-check','version':'1.0'},'capabilities':{'experimentalApi':True,'explicitGatewayOauth':True}})
        process.stdin.write(json.dumps({'method':'initialized','params':{}})+'\n');process.stdin.flush()
        result=request(2,'plugin/read',{'marketplacePath':str(market/'.agents/plugins/marketplace.json'),'pluginName':'trakt-mcp'})['plugin']
        names={s['name'].split(':')[-1] for s in result['skills']}
        assert names=={'what-to-watch','watching-profile','find-title','connection-help'},names
        assert result['mcpServers']==['trakt'],result['mcpServers']
        assert result['summary']['interface']['developerName']=='SwackTech LLC'
        assert not result['hooks']
        print('Codex package discovery passed: four skills, one MCP server, correct publisher; no personal installation or authentication performed.')
    finally:
        process.terminate()
        try:process.wait(timeout=10)
        except subprocess.TimeoutExpired:process.kill();process.wait()


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive',type=Path)
    parser.add_argument('--client',choices=('claude','codex','both'),default='both')
    args=parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='trakt-clean-clients-') as temp:
        temp=Path(temp);market=temp/'marketplace'
        for folder in ('.agents/plugins','.claude-plugin'):
            (market/folder).mkdir(parents=True)
            shutil.copy(ROOT/folder/'marketplace.json',market/folder/'marketplace.json')
        with zipfile.ZipFile(args.archive) as archive:
            for item in archive.infolist():
                target=(market/'plugins/trakt-mcp'/item.filename).resolve()
                if not target.is_relative_to((market/'plugins/trakt-mcp').resolve()):raise ValueError('Unsafe archive path')
                if (item.external_attr>>16)&0o170000==0o120000:raise ValueError('Symlink in archive')
            archive.extractall(market/'plugins/trakt-mcp')
        if args.client in ('claude','both'):
            env=os.environ|{'CLAUDE_CONFIG_DIR':str(temp/'claude-config'),'DISABLE_TELEMETRY':'1'}
            for command in (['plugin','marketplace','add',str(market)],['plugin','install','trakt-mcp@trakt-mcp'],['plugin','details','trakt-mcp@trakt-mcp']):
                result=subprocess.run(['claude',*command],cwd=temp,env=env,check=True,capture_output=True,text=True,timeout=90)
                if command[1]=='details':
                    for name in ('what-to-watch','watching-profile','find-title','connection-help'):assert name in result.stdout
                    assert 'MCP servers (1)' in result.stdout and 'Agents (0)' in result.stdout
                    print(result.stdout)
            print('Claude isolated installation and inventory passed; no authenticated workflow performed.')
        if args.client in ('codex','both'):codex_probe(market,temp)

if __name__=='__main__':main()
