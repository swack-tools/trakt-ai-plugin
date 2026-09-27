#!/usr/bin/env python3
"""Interactive device login. Saves plugin credentials privately; never prints tokens."""
import json, os, time, urllib.request, urllib.error
from pathlib import Path
BASE='https://trakt.swacktech.com'

def request(path,body,token=None):
    headers={'Content-Type':'application/json'}
    if token: headers['Authorization']='Bearer '+token
    r=urllib.request.Request(BASE+path,data=json.dumps(body).encode(),headers=headers)
    try:
        with urllib.request.urlopen(r,timeout=30) as response:
            return response.status,json.load(response)
    except urllib.error.HTTPError as e:
        return e.code,json.load(e)

def main():
    status,login=request('/auth/device/code',{})
    if status!=200:raise SystemExit('Login start failed: '+login.get('error','unknown'))
    print(login['instructions'])
    deadline=time.monotonic()+login['expires_in'];interval=login['interval']
    while time.monotonic()<deadline:
        time.sleep(interval)
        status,data=request('/auth/device/token',{'device_code':login['device_code']},login['session_token'])
        if status==200:
            directory=Path(os.environ.get('XDG_CONFIG_HOME',str(Path.home()/'.config')))/'trakt-mcp'
            directory.mkdir(parents=True,exist_ok=True,mode=0o700)
            path=directory/'credentials.json'
            fd=os.open(path,os.O_WRONLY|os.O_CREAT|os.O_TRUNC,0o600)
            os.fchmod(fd,0o600)
            with os.fdopen(fd,'w') as f:json.dump(data,f)
            print('Connected. Plugin credentials saved privately to '+str(path));return
        if data.get('error') in ('authorization_pending','slow_down'):
            interval=data.get('retry_after') or interval
        else:raise SystemExit('Login failed: '+data.get('error','unknown'))
    raise SystemExit('Code expired; run this script again.')
if __name__=='__main__':main()
