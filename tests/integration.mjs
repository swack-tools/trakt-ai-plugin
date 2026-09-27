import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,rm,writeFile,mkdir} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import {SSEClientTransport} from '@modelcontextprotocol/sdk/client/sse.js';
const base='http://127.0.0.1:8787';
const users=new Map();let next=0,refreshes=0,worker,mock;let logs='',stateDir; const requests=[];let deviceInterval=1;
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function api(path,{token,method='GET',body,headers={}}={}){const r=await fetch(base+path,{signal:AbortSignal.timeout(15000),method,headers:{...(token?{Authorization:`Bearer ${token}`} : {}),...(body?{'Content-Type':'application/json'}:{}),...headers},body:body?JSON.stringify(body):undefined});const text=await r.text();let data;try{data=JSON.parse(text);}catch{data=text;}return {status:r.status,data,headers:r.headers};}
async function device(interval=1){deviceInterval=interval;try{const r=await api('/auth/device/code',{method:'POST',body:{}});assert.equal(r.status,200,JSON.stringify(r.data));return r.data;}finally{deviceInterval=1;}}
async function connect(){const d=await device();users.get(d.device_code).authorized=true;await pause(1100);const r=await api('/auth/device/token',{token:d.session_token,method:'POST',body:{device_code:d.device_code}});assert.equal(r.status,200,JSON.stringify(r.data));assert.ok(r.data.access_token);return {...r.data,device:d};}
before(async()=>{
 mock=http.createServer(async(req,res)=>{let raw='';for await(const chunk of req)raw+=chunk;const body=raw?JSON.parse(raw):{};const u=new URL(req.url,'http://mock');requests.push({path:u.pathname,query:u.searchParams,headers:req.headers,body});
 assert.equal(req.headers['user-agent'],'trakt-mcp/1.0 (+https://trakt.swacktech.com)');assert.equal(req.headers['trakt-api-key'],'test-client-id');assert.equal(req.headers['trakt-api-version'],'2');
 let status=200,data;
 if(u.pathname==='/oauth/device/code'){const code=`device-${++next}`;users.set(code,{id:next,authorized:false});data={device_code:code,user_code:`USER${next}`,verification_url:'https://trakt.tv/activate',expires_in:600,interval:deviceInterval};}
 else if(u.pathname==='/oauth/device/token'){const user=users.get(body.code);assert.equal(body.client_secret,'test-client-secret');if(!user){status=404;data={};}else if(user.status){status=user.status;data={};}else if(!user.authorized){status=400;data={};}else{data={access_token:`trakt-${user.id}`,refresh_token:`refresh-${user.id}`,created_at:Math.floor(Date.now()/1000),expires_in:user.expired?1:3600};}}
 else if(u.pathname==='/oauth/token'){refreshes++;await pause(150);data={access_token:body.refresh_token.replace('refresh-','trakt-'),refresh_token:`rotated-${refreshes}`,created_at:Math.floor(Date.now()/1000),expires_in:3600};}
 else if(u.pathname.startsWith('/sync/watched/')){data=[{plays:1,movie:{title:'Private movie',ids:{trakt:Number(req.headers.authorization?.split('-').at(-1))},genres:['drama'],released:'2020-01-01'}}];}
 else if(u.pathname.startsWith('/recommendations/')){data=[{title:'Recommendation',genres:['drama'],year:2020}];}
 else if(u.pathname.startsWith('/search/')){res.setHeader('X-Pagination-Page','1');res.setHeader('X-Pagination-Page-Count','3');data=[{type:'movie',movie:{title:'A & B',year:2020,genres:['drama']}},{type:'show',show:{title:'Wrong year',year:1999,genres:['comedy']}}];}
 else {status=404;data={};}res.writeHead(status,{'Content-Type':'application/json'});res.end(JSON.stringify(data));});
 mock.listen(8799,'127.0.0.1');await once(mock,'listening');
 stateDir=await mkdtemp(join(tmpdir(),'trakt-mcp-test-'));
 worker=spawn('node',['node_modules/wrangler/bin/wrangler.js','dev','--config','tests/wrangler.test.toml','--port','8787','--inspector-port','9231','--persist-to',stateDir],{stdio:['ignore','pipe','pipe'],env:{...process.env,WRANGLER_SEND_METRICS:'false'}});worker.stdout.on('data',d=>{logs+=d});worker.stderr.on('data',d=>{logs+=d});
 for(let i=0;i<100;i++){try{const r=await api('/health');if(r.status===200)return;}catch{}await pause(200);}throw Error('Worker failed to start: '+logs);
});
after(async()=>{
 try {
  const directory=process.env.TRAKT_TEST_LOG_DIR||'.firecrawl';
  await mkdir(directory,{recursive:true});await writeFile(join(directory,'runtime.log'),logs);
 } finally {
  if(worker&&worker.exitCode===null&&worker.signalCode===null){
   const exited=once(worker,'exit');worker.kill('SIGTERM');
   const timer=setTimeout(()=>worker.kill('SIGKILL'),5000);timer.unref();
   try{await exited;}finally{clearTimeout(timer);}
  }
  mock?.closeAllConnections();await new Promise(r=>mock?mock.close(r):r());
  if(stateDir)await rm(stateDir,{recursive:true,force:true});
 }
});
test('discovery, origin, auth, and bounded input',async()=>{
 assert.equal((await api('/')).status,200);assert.equal((await api('/health')).status,200);assert.equal((await api('/sync/watched')).status,401);
 assert.match((await api('/sync/watched')).headers.get('www-authenticate'),/resource_metadata/);
 assert.equal((await api('/mcp',{method:'POST',headers:{Origin:'https://evil.test'},body:{}})).status,403);
 assert.equal((await api('/mcp',{method:'POST',body:{x:'x'.repeat(70000)}})).status,413);
 assert.equal((await api('/.well-known/oauth-authorization-server')).data.code_challenge_methods_supported[0],'S256');
 assert.equal((await api('/mcp')).status,405);
});
test('device pending, interval and cross-user isolation',async()=>{
 // The server uses whole seconds; leave room for requests crossing a second boundary.
 const a=await device(3),b=await device();
 const tokenRequests=()=>requests.filter(r=>r.path==='/oauth/device/token'&&r.body.code===a.device_code).length;
 const before=tokenRequests();
 assert.equal((await api('/auth/device/token',{method:'POST',token:a.session_token,body:{device_code:a.device_code}})).data.error,'slow_down');
 assert.equal(tokenRequests(),before,'early polls must not reach Trakt');
 assert.equal((await api('/auth/device/token',{method:'POST',token:a.session_token,body:{device_code:b.device_code}})).data.error,'invalid_device_code');
 await pause(a.interval*1000+100);const pending=await api('/auth/device/token',{method:'POST',token:a.session_token,body:{device_code:a.device_code}});assert.equal(pending.data.error,'authorization_pending');
 assert.equal(tokenRequests(),before+1,'a pending poll must reach Trakt after the interval');
 const forged=a.session_token.split('.')[0]+'.'+b.session_token.split('.')[1];assert.equal((await api('/search?query=test',{token:forged})).status,401);
 const user=await connect();const history=await api('/sync/watched?media_type=movies',{token:user.access_token});assert.equal(history.data.data[0].movie.ids.trakt,users.get(user.device.device_code).id);assert.ok(!JSON.stringify(history.data).includes('refresh_token'));
 assert.equal((await api('/sync/watched',{token:b.session_token})).data.error,'trakt_login_required');
});
test('search encoding, filters, pagination, recommendations',async()=>{
 const u=await connect();const r=await api('/search?query=A%20%26%20B&genres=drama&years=2020&limit=5',{token:u.access_token});assert.equal(r.data.data.length,1);assert.equal(r.data.pagination.page_count,3);assert.equal(r.data.filters_applied_to_page,true);
 assert.equal(requests.at(-1).query.get('query'),'A & B');assert.equal(requests.at(-1).headers.authorization,undefined);
 assert.equal((await api('/recommendations?media_type=show&genres=drama&years=2020&limit=3',{token:u.access_token})).status,200);assert.equal(requests.at(-1).path,'/recommendations/shows');assert.equal(requests.at(-1).query.get('genres'),'drama');
 assert.equal((await api('/search?query=test&limit=101',{token:u.access_token})).status,400);
});
test('real MCP SDK initializes and calls both transports',async()=>{
 const u=await connect();for(const kind of ['http','sse']){const client=new Client({name:'integration',version:'1.0.0'});const headers={Authorization:`Bearer ${u.access_token}`};const transport=kind==='http'?new StreamableHTTPClientTransport(new URL(base+'/mcp'),{requestInit:{headers}}):new SSEClientTransport(new URL(base+'/sse'),{requestInit:{headers},eventSourceInit:{fetch:(url,init)=>fetch(url,{...init,headers:{...init?.headers,...headers}})}});
 try{await client.connect(transport);assert.equal((await client.listTools()).tools.length,5);const result=await client.callTool({name:'trakt_search',arguments:{query:'A & B'}});assert.equal(result.isError,false);}finally{await client.close();}}
 const notify=await api('/mcp',{method:'POST',token:u.access_token,body:{jsonrpc:'2.0',method:'notifications/initialized'}});assert.equal(notify.status,202);assert.equal(notify.data,'');
 assert.equal((await api('/mcp',{method:'POST',token:u.access_token,body:{jsonrpc:'2.0',id:1,method:'unknown'}})).data.error.code,-32601);
});
test('single-use Trakt refresh is serialized and plugin refresh rotates',async()=>{
 const d=await device();users.get(d.device_code).authorized=true;users.get(d.device_code).expired=true;await pause(1100);const u=(await api('/auth/device/token',{method:'POST',token:d.session_token,body:{device_code:d.device_code}})).data;const before=refreshes;
 const rs=await Promise.all(Array.from({length:3},()=>api('/sync/watched?media_type=movies',{token:u.access_token})));rs.forEach(r=>assert.equal(r.status,200,JSON.stringify(r.data)));assert.equal(refreshes,before+1);
 const next=await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:u.refresh_token}});assert.equal(next.status,200);assert.notEqual(next.data.refresh_token,u.refresh_token);
 const invalid=next.data.refresh_token.split('.')[0]+'.'+'f'.repeat(64);assert.equal((await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:invalid}})).status,400);assert.equal((await api('/search?query=x',{token:next.data.access_token})).status,200);
 assert.equal((await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:u.refresh_token}})).status,400);
 assert.equal((await api('/search?query=x',{token:u.access_token})).status,401);
 assert.equal((await api('/search?query=x',{token:next.data.access_token})).status,401);
 const fresh=await connect();assert.equal((await api('/auth/session',{method:'DELETE',token:fresh.access_token})).status,204);
 assert.equal((await api('/search?query=x',{token:next.data.access_token})).status,401);
});
test('OAuth registration, browser consent, PKCE, redirect binding and code replay',async()=>{
 const registration=await api('/oauth/register',{method:'POST',body:{client_name:'SDK test',redirect_uris:['http://127.0.0.1:9999/callback'],token_endpoint_auth_method:'none'}});assert.equal(registration.status,201,JSON.stringify(registration.data));const client_id=registration.data.client_id;
 const {selectResourceURL}=await import('@modelcontextprotocol/sdk/client/auth.js');const resource=(await selectResourceURL(new URL('https://trakt.swacktech.com/sse'),{},(await api('/.well-known/oauth-protected-resource')).data)).href;
 const verifier='v'.repeat(64),challenge=createHash('sha256').update(verifier).digest('base64url');const params=new URLSearchParams({client_id,redirect_uri:'http://127.0.0.1:9999/callback',response_type:'code',state:'state-123',code_challenge:challenge,code_challenge_method:'S256',resource});
 const page=await api('/oauth/authorize?'+params);assert.equal(page.status,200,JSON.stringify(page.data));const session_id=page.data.match(/data-session="([^"]+)"/)[1],ticket=page.data.match(/data-ticket="([^"]+)"/)[1];
 const d=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'start'}});assert.equal(d.status,200);users.get(d.data.device_code).authorized=true;await pause(1100);
 const complete=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'poll'}});assert.equal(complete.status,200);const redirect=new URL(complete.data.redirect);assert.equal(redirect.searchParams.get('state'),'state-123');
 const body={grant_type:'authorization_code',client_id,code:redirect.searchParams.get('code'),redirect_uri:'http://127.0.0.1:9999/callback',code_verifier:verifier,resource};
 assert.equal((await api('/oauth/token',{method:'POST',body:{...body,code_verifier:'x'.repeat(64)}})).status,400);
 assert.equal((await api('/oauth/token',{method:'POST',body:{...body,redirect_uri:'https://evil.test'}})).status,400);
 const tokens=await api('/oauth/token',{method:'POST',body});assert.equal(tokens.status,200);assert.equal((await api('/oauth/token',{method:'POST',body})).status,400);
 assert.equal((await api('/sync/watched?media_type=movies',{token:tokens.data.access_token})).status,200);
});
test('both transports negotiate the advertised OAuth resource using the SDK',async()=>{
 const {selectResourceURL}=await import('@modelcontextprotocol/sdk/client/auth.js');
 const metadata=(await api('/.well-known/oauth-protected-resource')).data;
 for(const path of ['/mcp','/sse'])assert.equal(String(await selectResourceURL(new URL('https://trakt.swacktech.com'+path),{},metadata)),'https://trakt.swacktech.com/');
});

test('temporary tokens cannot search and stale SSE channels cause no side effects',async()=>{
 const d=await device();assert.equal((await api('/search?query=x',{token:d.session_token})).status,401);
 const u=await connect();const before=next;const body={jsonrpc:'2.0',id:1,method:'tools/call',params:{name:'trakt_request_login',arguments:{}}};
 assert.equal((await api('/messages?session_id=missing',{method:'POST',token:u.access_token,body})).status,404);assert.equal(next,before);
});
