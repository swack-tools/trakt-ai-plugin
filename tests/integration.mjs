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
const watchedFixtures=new Map();
const users=new Map();let next=0,refreshes=0,worker,mock;let logs='',stateDir; const requests=[];let deviceInterval=1;
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function api(path,{token,method='GET',body,headers={}}={}){const r=await fetch(base+path,{signal:AbortSignal.timeout(15000),method,headers:{...(token?{Authorization:`Bearer ${token}`} : {}),...(body?{'Content-Type':'application/json'}:{}),...headers},body:body?JSON.stringify(body):undefined});const text=await r.text();let data;try{data=JSON.parse(text);}catch{data=text;}return {status:r.status,data,headers:r.headers};}
async function device(interval=1){deviceInterval=interval;try{const r=await api('/auth/device/code',{method:'POST',body:{}});assert.equal(r.status,200,JSON.stringify(r.data));return r.data;}finally{deviceInterval=1;}}
async function connect(){const d=await device();users.get(d.device_code).authorized=true;await pause(1100);const r=await api('/auth/device/token',{token:d.session_token,method:'POST',body:{device_code:d.device_code}});assert.equal(r.status,200,JSON.stringify(r.data));assert.ok(r.data.access_token);return {...r.data,device:d};}
// Pagination cases share a fixture account so this suite does not exhaust the
// production IP-based device-registration limit. Authentication tests keep separate users.
let paginationAccount;
async function paginationUser(){return paginationAccount??=await connect();}
function paginationHeaders(res,{page,limit,item_count,page_count=Math.ceil(item_count/limit)},override={}){
 const headers={'X-Pagination-Page':page,'X-Pagination-Page-Count':page_count,'X-Pagination-Limit':limit,'X-Pagination-Item-Count':item_count,...override};
 for(const [name,value] of Object.entries(headers))if(value!==null)res.setHeader(name,String(value));
}
function summaries(media,count){const type=media==='movies'?'movie':'show';return Array.from({length:count},(_,i)=>({plays:i%3+1,[type]:{title:`Fixture ${type} ${i+1}`,ids:{trakt:i+1},genres:['drama'],year:2020}}));}
function events(count){return Array.from({length:count},(_,i)=>({id:1000+i,watched_at:new Date(Date.UTC(2025,0,1)-i*60000).toISOString(),action:'watch',type:'movie',movie:{title:`Repeat movie ${i%7}`,ids:{trakt:i%7+1}}}));}
function fixtureFor(user,fixture){watchedFixtures.set(users.get(user.device.device_code).id,fixture);}
function watchedRequests(start=0){return requests.slice(start).filter(r=>/^\/sync\/(watched|history)\//.test(r.path));}
before(async()=>{
 mock=http.createServer(async(req,res)=>{let raw='';for await(const chunk of req)raw+=chunk;const body=raw?JSON.parse(raw):{};const u=new URL(req.url,'http://mock');requests.push({path:u.pathname,query:u.searchParams,headers:req.headers,body});
 assert.equal(req.headers['user-agent'],'trakt-mcp/1.0 (+https://plugin.example.test)');assert.equal(req.headers['trakt-api-key'],'test-client-id');assert.equal(req.headers['trakt-api-version'],'2');
 let status=200,data;
 if(u.pathname==='/oauth/device/code'){const code=`device-${++next}`;users.set(code,{id:next,authorized:false});data={device_code:code,user_code:`USER${next}`,verification_url:'https://trakt.tv/activate',expires_in:600,interval:deviceInterval};}
 else if(u.pathname==='/oauth/device/token'){const user=users.get(body.code);assert.equal(body.client_secret,'test-client-secret');if(!user){status=404;data={};}else if(user.status){status=user.status;data={};}else if(!user.authorized){status=400;data={};}else{data={access_token:`trakt-${user.id}`,refresh_token:`refresh-${user.id}`,created_at:Math.floor(Date.now()/1000),expires_in:user.expired?1:3600};}}
 else if(u.pathname==='/oauth/token'){refreshes++;await pause(150);data={access_token:body.refresh_token.replace('refresh-','trakt-'),refresh_token:`rotated-${refreshes}`,created_at:Math.floor(Date.now()/1000),expires_in:3600};}
 else if(/^\/sync\/(watched|history)\//.test(u.pathname)){
  const userId=Number(req.headers.authorization?.split('-').at(-1)),fixture=watchedFixtures.get(userId);
  if(!fixture){data=[{plays:1,movie:{title:'Private movie',ids:{trakt:userId},genres:['drama'],released:'2020-01-01'}}];}
  else {
   const media=u.pathname.split('/').at(-1),page=Number(u.searchParams.get('page')||1),limit=fixture.limit||Number(u.searchParams.get('limit')||100);
   const rows=u.pathname.includes('/history/')?(fixture.recent?.[media]||[]):(fixture[media]||[]);
   if(fixture.failPage===page){status=fixture.failStatus;data={private_upstream_detail:'must not be returned'};if(status===429)res.setHeader('Retry-After','9');}
   else {data=fixture.noHeaders?rows:rows.slice((page-1)*limit,page*limit);if(!fixture.noHeaders)paginationHeaders(res,{page,limit,item_count:rows.length},fixture.headers);}
  }
 }
 else if(u.pathname.startsWith('/recommendations/')){data=[{title:'Recommendation',genres:['drama'],year:2020}];}
 else if(u.pathname.startsWith('/search/') && u.searchParams.get('query')==='rate-limit'){status=429;res.setHeader('Retry-After','7');data={private_upstream_detail:'must not be returned'};}
 else if(u.pathname.startsWith('/search/') && u.searchParams.get('query')==='upstream-error'){status=503;data={private_upstream_detail:'must not be returned'};}
 else if(u.pathname.startsWith('/search/')){if(u.searchParams.get('query')!=='no-headers')paginationHeaders(res,{page:Number(u.searchParams.get('page')||1),limit:Number(u.searchParams.get('limit')||20),page_count:3,item_count:3*Number(u.searchParams.get('limit')||20)});data=[{type:'movie',movie:{title:'A & B',year:2020,genres:['drama']}},{type:'show',show:{title:'Wrong year',year:1999,genres:['comedy']}}];}
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
 assert.equal((await api('/sync/watched')).headers.get('www-authenticate'),'Bearer resource_metadata="https://plugin.example.test/.well-known/oauth-protected-resource"');
 assert.equal((await api('/health',{headers:{Origin:'https://plugin.example.test'}})).headers.get('access-control-allow-origin'),'https://plugin.example.test');
 const schema=(await api('/openapi.json')).data;assert.equal(schema.servers[0].url,'https://plugin.example.test');assert.equal(schema.components.securitySchemes.oauth.flows.authorizationCode.authorizationUrl,'https://plugin.example.test/oauth/authorize');
 assert.equal((await api('/.well-known/ai-plugin.json')).status,404,'legacy metadata must not invent a support address');
 const challenge=await api('/.well-known/openai-apps-challenge');assert.equal(challenge.data,'fixture-domain-challenge');assert.match(challenge.headers.get('content-type'),/^text\/plain/);
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
test('watched summaries traverse 251 movies in exactly three caller-requested pages',async()=>{
 const u=await paginationUser(),rows=summaries('movies',251);fixtureFor(u,{movies:rows});const start=requests.length,seen=[];
 for(const page of [1,2,3]){
  const r=await api(`/sync/watched?media_type=movies&page=${page}`,{token:u.access_token});assert.equal(r.status,200,JSON.stringify(r.data));
  assert.deepEqual(r.data.pagination,{page,page_count:3,limit:100,item_count:251,has_more:page<3,next_page:page<3?page+1:null});
  assert.equal(r.data.data.length,page===3?51:100);seen.push(...r.data.data);
  assert.equal(watchedRequests(start).length,page,'one upstream request per requested movie page');
 }
 assert.deepEqual(seen,rows);assert.equal(new Set(seen.map(r=>r.movie.ids.trakt)).size,251);
 for(const [index,request] of watchedRequests(start).entries()){
  assert.equal(request.path,'/sync/watched/movies');assert.equal(request.query.get('extended'),'full');assert.equal(request.query.get('page'),String(index+1));assert.equal(request.query.get('limit'),'100');
  assert.equal(request.headers.authorization,`Bearer trakt-${users.get(u.device.device_code).id}`);
 }
});
test('compact traversal preserves viewing evidence while full metadata stays opt-in',async()=>{
 const u=await paginationUser(),rows=summaries('movies',2);rows[0].movie.overview='Long plot';rows[0].movie.images={poster:'https://example.test/poster'};fixtureFor(u,{movies:rows});
 const compact=await api('/sync/watched?media_type=movie',{token:u.access_token});assert.equal(compact.status,200);assert.equal(compact.data.data[0].movie.overview,undefined);assert.equal(compact.data.data[0].movie.images,undefined);assert.equal(compact.data.data[0].plays,1);assert.deepEqual(compact.data.data[0].movie.ids,rows[0].movie.ids);assert.deepEqual(compact.data.data[0].movie.genres,['drama']);
 const full=await api('/sync/watched?media_type=movie&detail=full',{token:u.access_token});assert.equal(full.status,200);assert.deepEqual(full.data.data,rows);assert.equal(requests.at(-1).query.has('detail'),false);
 for(const path of ['/sync/watched?detail=unknown','/recommendations?detail=compact','/search?query=x&detail=compact'])assert.equal((await api(path,{token:u.access_token})).status,400);
});
test('recent returns exactly the first 100 ordered play events and retains rewatches',async()=>{
 const u=await paginationUser(),rows=events(130);fixtureFor(u,{recent:{movies:rows}});const start=requests.length;
 const r=await api('/sync/watched?media_type=movie&mode=recent&limit=100',{token:u.access_token});assert.equal(r.status,200,JSON.stringify(r.data));
 assert.deepEqual(r.data.data,rows.slice(0,100));assert.equal(r.data.data.length,100);assert.equal(new Set(r.data.data.map(r=>r.movie.ids.trakt)).size,7);
 assert.ok(r.data.data.every((r,i,a)=>i===0||r.watched_at<a[i-1].watched_at));
 assert.deepEqual(r.data.pagination,{page:1,page_count:2,limit:100,item_count:130,has_more:true,next_page:2});
 const calls=watchedRequests(start);assert.equal(calls.length,1);assert.equal(calls[0].path,'/sync/history/movies');assert.equal(calls[0].query.get('extended'),'full');assert.equal(calls[0].query.get('page'),'1');assert.equal(calls[0].query.get('limit'),'100');
});
test('upstream effective limits determine traversal without skipped rows',async()=>{
 const u=await paginationUser(),rows=summaries('movies',251);fixtureFor(u,{movies:rows,limit:40});const start=requests.length,seen=[];
 for(let page=1;page<=7;page++){
  const r=await api(`/sync/watched?media_type=movies&limit=100&page=${page}`,{token:u.access_token});assert.equal(r.status,200,JSON.stringify(r.data));
  assert.deepEqual(r.data.pagination,{page,page_count:7,limit:40,item_count:251,has_more:page<7,next_page:page<7?page+1:null});seen.push(...r.data.data);
 }
 assert.deepEqual(seen,rows);assert.equal(watchedRequests(start).length,7);
});
test('movie and show pagination remain independent for both watched modes',async()=>{
 const u=await paginationUser(),movies=summaries('movies',251),shows=summaries('shows',105);
 const recentMovies=events(251),recentShows=events(105).map(({movie,...row})=>({...row,type:'episode',show:{...movie,title:'Fixture show'},episode:{season:1,number:1,ids:{trakt:42}}}));
 fixtureFor(u,{movies,shows,recent:{movies:recentMovies,shows:recentShows}});
 for(const [mode,movieRows,showRows] of [['all',movies,shows],['recent',recentMovies,recentShows]]){
  const start=requests.length,r=await api(`/sync/watched?mode=${mode}&page=2&limit=100`,{token:u.access_token});assert.equal(r.status,200,JSON.stringify(r.data));
  assert.deepEqual(r.data.movies.data,movieRows.slice(100,200));assert.deepEqual(r.data.shows.data,showRows.slice(100,200));
  assert.equal(r.data.movies.pagination.next_page,3);assert.equal(r.data.movies.pagination.has_more,true);assert.equal(r.data.shows.pagination.next_page,null);assert.equal(r.data.shows.pagination.has_more,false);
  assert.equal(r.data.movies.pagination.item_count,251);assert.equal(r.data.shows.pagination.item_count,105);
  const calls=watchedRequests(start);assert.equal(calls.length,2);assert.deepEqual(calls.map(r=>r.path).sort(),[`/sync/${mode==='all'?'watched':'history'}/movies`,`/sync/${mode==='all'?'watched':'history'}/shows`]);
 }
});
test('late pagination failures leave prior pages usable and preserve retry guidance',async()=>{
 const u=await paginationUser(),rows=summaries('movies',251);
 for(const status of [429,503]){
  fixtureFor(u,{movies:rows,failPage:3,failStatus:status});const start=requests.length,partial=[];
  for(const page of [1,2]){const r=await api(`/sync/watched?media_type=movies&page=${page}`,{token:u.access_token});assert.equal(r.status,200);partial.push(...r.data.data);}
  const failed=await api('/sync/watched?media_type=movies&page=3',{token:u.access_token});
  assert.equal(failed.status,status===429?429:502);assert.equal(failed.data.error,status===429?'trakt_rate_limited':'trakt_upstream_error');assert.equal(failed.data.retry_after,status===429?9:null);
  assert.deepEqual(partial,rows.slice(0,200));assert.equal(failed.data.data,undefined);assert.ok(!JSON.stringify(failed.data).includes('private_upstream_detail'));assert.equal(watchedRequests(start).length,3,'no automatic retry or page loop');
 }
});
test('watched whole-list fallback, empty accounts, and missing recent pagination are explicit',async()=>{
 const u=await paginationUser(),rows=summaries('movies',251);fixtureFor(u,{movies:rows,noHeaders:true});const start=requests.length;
 const whole=await api('/sync/watched?media_type=movies&limit=100',{token:u.access_token});assert.equal(whole.status,200);assert.deepEqual(whole.data.data,rows);
 assert.equal(whole.data.pagination.page,1);assert.equal(whole.data.pagination.page_count,1);assert.equal(whole.data.pagination.item_count,251);assert.equal(whole.data.pagination.has_more,false);assert.equal(whole.data.pagination.next_page,null);
 const repeated=await api('/sync/watched?media_type=movies&page=2',{token:u.access_token});assert.equal(repeated.status,502);assert.equal(repeated.data.error,'invalid_trakt_pagination');assert.equal(watchedRequests(start).length,2);
 for(const noHeaders of [false,true]){
  fixtureFor(u,{movies:[],noHeaders});const empty=await api('/sync/watched?media_type=movies',{token:u.access_token});assert.equal(empty.status,200,JSON.stringify(empty.data));assert.deepEqual(empty.data.data,[]);assert.equal(empty.data.pagination.item_count,0);assert.equal(empty.data.pagination.has_more,false);assert.equal(empty.data.pagination.next_page,null);
 }
 fixtureFor(u,{recent:{movies:events(3)},noHeaders:true});const recent=await api('/sync/watched?media_type=movies&mode=recent',{token:u.access_token});assert.equal(recent.status,502);assert.equal(recent.data.error,'invalid_trakt_pagination');
});
test('malformed, incomplete, and ignored-page metadata fail closed',async()=>{
 const u=await paginationUser(),rows=summaries('movies',251);
 for(const headers of [
  {'X-Pagination-Limit':null},{'X-Pagination-Page':'oops'},{'X-Pagination-Page':'0'},
  {'X-Pagination-Page':'1'},{'X-Pagination-Limit':'0'},{'X-Pagination-Item-Count':'-1'},
  {'X-Pagination-Page-Count':'oops'}
 ]){
  fixtureFor(u,{movies:rows,headers});const start=requests.length,r=await api('/sync/watched?media_type=movies&page=2',{token:u.access_token});
  assert.equal(r.status,502,JSON.stringify({headers,response:r.data}));assert.equal(r.data.error,'invalid_trakt_pagination');assert.equal(watchedRequests(start).length,1);
 }
});
test('pagination inputs stay endpoint-specific and filtered empty search pages can continue',async()=>{
 const u=await paginationUser();
 for(const path of ['/sync/watched?page=0','/sync/watched?limit=0','/sync/watched?limit=101','/sync/watched?mode=unknown','/recommendations?page=2','/recommendations?mode=recent','/search?query=x&mode=all']){
  const start=requests.length;assert.equal((await api(path,{token:u.access_token})).status,400,path);assert.equal(requests.length,start,'invalid inputs must not call Trakt');
 }
 const filtered=await api('/search?query=A&genres=horror&page=2&limit=5',{token:u.access_token});assert.equal(filtered.status,200);assert.deepEqual(filtered.data.data,[]);assert.equal(filtered.data.pagination.page,2);assert.equal(filtered.data.pagination.has_more,true);assert.equal(filtered.data.pagination.next_page,3);assert.equal(filtered.data.filters_applied_to_page,true);
 const unknown=await api('/search?query=no-headers',{token:u.access_token});assert.equal(unknown.status,200);assert.equal(unknown.data.pagination.has_more,null);assert.equal(unknown.data.pagination.next_page,null);
});
test('real MCP SDK initializes and calls both transports',async()=>{
 const u=await connect();fixtureFor(u,{movies:summaries('movies',251),recent:{movies:events(130)}});for(const kind of ['http','sse']){const client=new Client({name:'integration',version:'1.0.0'});const headers={Authorization:`Bearer ${u.access_token}`};const transport=kind==='http'?new StreamableHTTPClientTransport(new URL(base+'/mcp'),{requestInit:{headers}}):new SSEClientTransport(new URL(base+'/sse'),{requestInit:{headers},eventSourceInit:{fetch:(url,init)=>fetch(url,{...init,headers:{...init?.headers,...headers}})}});
 try{await client.connect(transport);assert.equal((await client.listTools()).tools.length,5);const result=await client.callTool({name:'trakt_search',arguments:{query:'A & B'}});assert.equal(result.isError,false);
 const start=requests.length;
 for(const [mode,page,limit,expectedLength] of [['all',2,100,100],['recent',2,100,30]]){
  const watched=await client.callTool({name:'trakt_get_watched_history',arguments:{media_type:'movies',mode,page,limit}});assert.equal(watched.isError,false,JSON.stringify(watched));
  const data=JSON.parse(watched.content[0].text);assert.equal(data.pagination.page,page);assert.equal(data.data.length,expectedLength);assert.equal(requests.at(-1).query.get('page'),'2');assert.equal(requests.at(-1).path,mode==='all'?'/sync/watched/movies':'/sync/history/movies');
 }
 assert.equal(watchedRequests(start).length,2);
 }finally{await client.close();}}
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
 const fresh=await connect();assert.equal((await api('/auth/session',{method:'DELETE',token:fresh.access_token})).status,204);assert.equal((await api('/search?query=x',{token:fresh.access_token})).status,401);assert.equal((await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:fresh.refresh_token}})).status,400);
 assert.equal((await api('/search?query=x',{token:next.data.access_token})).status,401);
});
test('OAuth registration, browser consent, PKCE, redirect binding and code replay',async()=>{
 const registration=await api('/oauth/register',{method:'POST',body:{client_name:'SDK test',redirect_uris:['http://127.0.0.1:9999/callback'],token_endpoint_auth_method:'none'}});assert.equal(registration.status,201,JSON.stringify(registration.data));const client_id=registration.data.client_id;
 const {selectResourceURL}=await import('@modelcontextprotocol/sdk/client/auth.js');const resource=(await selectResourceURL(new URL('https://plugin.example.test/sse'),{},(await api('/.well-known/oauth-protected-resource')).data)).href;
 const verifier='v'.repeat(64),challenge=createHash('sha256').update(verifier).digest('base64url');const params=new URLSearchParams({client_id,redirect_uri:'http://127.0.0.1:9999/callback',response_type:'code',state:'state-123',code_challenge:challenge,code_challenge_method:'S256',resource});
 const page=await api('/oauth/authorize?'+params);assert.equal(page.status,200,JSON.stringify(page.data));const session_id=page.data.match(/data-session="([^"]+)"/)[1],ticket=page.data.match(/data-ticket="([^"]+)"/)[1];
 assert.equal((await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'unknown'}})).status,400);
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
 for(const path of ['/mcp','/sse'])assert.equal(String(await selectResourceURL(new URL('https://plugin.example.test'+path),{},metadata)),'https://plugin.example.test/');
});

test('temporary tokens cannot search and stale SSE channels cause no side effects',async()=>{
 const d=await device();assert.equal((await api('/search?query=x',{token:d.session_token})).status,401);
 const u=await connect();const before=next;const body={jsonrpc:'2.0',id:1,method:'tools/call',params:{name:'trakt_request_login',arguments:{}}};
 assert.equal((await api('/messages?session_id=missing',{method:'POST',token:u.access_token,body})).status,404);assert.equal(next,before);
});

test('tool failures preserve bounded retry guidance without upstream error bodies',async()=>{
 const u=await connect();
 const call=query=>api('/mcp',{method:'POST',token:u.access_token,body:{jsonrpc:'2.0',id:7,method:'tools/call',params:{name:'trakt_search',arguments:{query}}}});
 for(const [query,error,retry] of [['rate-limit','trakt_rate_limited',7],['upstream-error','trakt_upstream_error',null]]){
  const response=await call(query);assert.equal(response.status,200);assert.equal(response.data.result.isError,true);
  const data=JSON.parse(response.data.result.content[0].text);assert.equal(data.error,error);assert.equal(data.retry_after,retry);assert.ok(!JSON.stringify(response.data).includes('private_upstream_detail'));
 }
});
