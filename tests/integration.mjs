import {test,before,after} from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp,rm,writeFile,mkdir,readFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {Client} from '@modelcontextprotocol/sdk/client/index.js';
import {StreamableHTTPClientTransport} from '@modelcontextprotocol/sdk/client/streamableHttp.js';
import {SSEClientTransport} from '@modelcontextprotocol/sdk/client/sse.js';
const base='http://127.0.0.1:8787';
const watchedFixtures=new Map();
const mockLists=new Map(),upstreamFailures=new Map();let nextList=9000;
const operationCatalog=JSON.parse(await readFile(new URL('../api/trakt/catalog.json',import.meta.url),'utf8'));
const users=new Map();let next=0,refreshes=0,worker,mock;let logs='',stateDir; const requests=[];let deviceInterval=1;
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function api(path,{token,method='GET',body,headers={}}={}){const r=await fetch(base+path,{signal:AbortSignal.timeout(15000),method,headers:{...(token?{Authorization:`Bearer ${token}`} : {}),...(body?{'Content-Type':'application/json'}:{}),...headers},body:body?JSON.stringify(body):undefined});const text=await r.text();let data;try{data=JSON.parse(text);}catch{data=text;}return {status:r.status,data,headers:r.headers};}
async function device(interval=1){deviceInterval=interval;try{const r=await api('/auth/device/code',{method:'POST',body:{}});assert.equal(r.status,200,JSON.stringify(r.data));return r.data;}finally{deviceInterval=1;}}
async function connect(){const d=await device();users.get(d.device_code).authorized=true;await pause(1100);const r=await api('/auth/device/token',{token:d.session_token,method:'POST',body:{device_code:d.device_code}});assert.equal(r.status,200,JSON.stringify(r.data));assert.ok(r.data.access_token);assert.equal(r.data.scope,'trakt:read');return {...r.data,device:d};}
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
 mock=http.createServer(async(req,res)=>{let raw='';for await(const chunk of req)raw+=chunk;const body=raw?JSON.parse(raw):{};const u=new URL(req.url,'http://mock');requests.push({method:req.method,path:u.pathname,query:u.searchParams,headers:req.headers,body});
 assert.equal(req.headers['user-agent'],'trakt-mcp/1.0 (+https://plugin.example.test)');assert.equal(req.headers['trakt-api-key'],'test-client-id');assert.equal(req.headers['trakt-api-version'],'2');
 let status=200,data;
 if(u.pathname==='/oauth/device/code'){const code=`device-${++next}`;users.set(code,{id:next,authorized:false});data={device_code:code,user_code:`USER${next}`,verification_url:'https://trakt.tv/activate',expires_in:600,interval:deviceInterval};}
 else if(u.pathname==='/oauth/device/token'){const user=users.get(body.code);assert.equal(body.client_secret,'test-client-secret');if(!user){status=404;data={};}else if(user.status){status=user.status;data={};}else if(!user.authorized){status=400;data={};}else{data={access_token:`trakt-${user.id}`,refresh_token:`refresh-${user.id}`,created_at:Math.floor(Date.now()/1000),expires_in:user.expired?1:3600};}}
 else if(u.pathname==='/oauth/token'){refreshes++;await pause(150);data={access_token:body.refresh_token.replace('refresh-','trakt-'),refresh_token:`rotated-${refreshes}`,created_at:Math.floor(Date.now()/1000),expires_in:3600};}
 else if(upstreamFailures.has(`${req.method} ${u.pathname}`)&&!upstreamFailures.get(`${req.method} ${u.pathname}`).commit){
  const failure=upstreamFailures.get(`${req.method} ${u.pathname}`);status=failure.status;data=failure.data??{private_upstream_detail:'must not be returned'};if(failure.pagination)paginationHeaders(res,failure.pagination);if(status===429)res.setHeader('Retry-After','13');
 }
 else if(u.pathname==='/users/me/lists'&&req.method==='POST'){
  const id=++nextList;data={name:body.name,privacy:body.privacy,ids:{trakt:id,slug:`fixture-${id}`}};
  mockLists.set(String(id),{...data,owner:req.headers.authorization,items:[]});status=201;
  if(upstreamFailures.get(`${req.method} ${u.pathname}`)?.commit){status=503;data={private_upstream_detail:'committed, then response failed'};}
 }
 else if(/^\/users\/me\/lists\/\d+\/items$/.test(u.pathname)&&req.method==='POST'){
  const list=mockLists.get(u.pathname.split('/')[4]);
  if(!list){status=404;data={};}else if(list.owner!==req.headers.authorization){status=403;data={};}else{
   list.items.push(...(body.movies||[]).map(movie=>({type:'movie',movie})));status=201;data={added:{movies:(body.movies||[]).length}};
  }
 }
 else if(/^\/users\/me\/lists\/\d+\/items\//.test(u.pathname)&&req.method==='GET'){
  const list=mockLists.get(u.pathname.split('/')[4]);
  if(!list){status=404;data={};}else if(list.owner!==req.headers.authorization){status=403;data={};}else{
   data=list.items;paginationHeaders(res,{page:1,limit:100,item_count:data.length});
  }
 }
 else if(/^\/users\/me\/lists\/\d+\/$/.test(u.pathname)&&req.method==='DELETE'){
  const id=u.pathname.split('/')[4],list=mockLists.get(id);
  if(!list){status=404;data={};}else if(list.owner!==req.headers.authorization){status=403;data={};}else{mockLists.delete(id);status=204;data=null;}
 }
 else if(u.pathname==='/lists/popular'&&req.method==='GET'){
  const page=Number(u.searchParams.get('page')||1),limit=1;
  data=page===1?[{like_count:42,list:{name:'Public discoveries',ids:{trakt:77},privacy:'public'}}]:[{like_count:9,list:{name:'Second page',ids:{trakt:78},privacy:'public'}}];
  paginationHeaders(res,{page,limit,page_count:2,item_count:limit+1});
 }
 else if(/^\/calendars\/(my|all)\/movies\/2026-09-27\/7$/.test(u.pathname)){
  data=[{released:'2026-09-29',movie:{title:u.pathname.includes('/my/')?'Personal upcoming':'Public upcoming',ids:{trakt:123}}}];
 }
 else if(/^\/sync\/(watched|history)\//.test(u.pathname)){
  const userId=Number(req.headers.authorization?.split('-').at(-1)),fixture=watchedFixtures.get(userId);
  if(!fixture){data=[{plays:1,movie:{title:'Private movie',ids:{trakt:userId},genres:['drama'],released:'2020-01-01'}}];}
  else {
   const media=u.pathname.split('/').at(-1),page=Number(u.searchParams.get('page')||1),limit=fixture.limit||Number(u.searchParams.get('limit')||100);
   const rows=u.pathname.includes('/history/')?(fixture.recent?.[media]||[]):(fixture[media]||[]);
   if(fixture.failPage===page){status=fixture.failStatus;data={private_upstream_detail:'must not be returned'};if(status===429)res.setHeader('Retry-After','9');}
   else {data=fixture.noHeaders?rows:rows.slice((page-1)*limit,page*limit);if(fixture.truncatePage===page)data=data.slice(0,fixture.truncatedLength);if(!fixture.noHeaders)paginationHeaders(res,{page,limit,item_count:rows.length},fixture.headers);}
  }
 }
 else if(u.pathname.startsWith('/recommendations/')){data=[{title:'Recommendation',genres:['drama'],year:2020}];}
 else if(u.pathname.startsWith('/search/') && u.searchParams.get('query')==='rate-limit'){status=429;res.setHeader('Retry-After','7');data={private_upstream_detail:'must not be returned'};}
 else if(u.pathname.startsWith('/search/') && u.searchParams.get('query')==='upstream-error'){status=503;data={private_upstream_detail:'must not be returned'};}
 else if(u.pathname.startsWith('/search/')){const effectiveLimit=Math.min(2,Number(u.searchParams.get('limit')||20));if(u.searchParams.get('query')!=='no-headers')paginationHeaders(res,{page:Number(u.searchParams.get('page')||1),limit:effectiveLimit,page_count:3,item_count:3*effectiveLimit});data=[{type:'movie',movie:{title:'A & B',year:2020,genres:['drama']}},{type:'show',show:{title:'Wrong year',year:1999,genres:['comedy']}}].slice(0,Number(u.searchParams.get('limit')||20));}
 else {status=404;data={};}res.writeHead(status,{'Content-Type':'application/json'});res.end(upstreamFailures.get(`${req.method} ${u.pathname}`)?.raw??JSON.stringify(data));});
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
 const filtered=await api('/search?query=A&genres=horror&page=2&limit=5',{token:u.access_token});assert.equal(filtered.status,200);assert.deepEqual(filtered.data.data,[]);assert.equal(filtered.data.pagination.page,2);assert.equal(filtered.data.pagination.has_more,true);assert.equal(filtered.data.pagination.next_page,3);assert.equal(filtered.data.filters_applied_to_page,true);assert.equal(filtered.data.pagination.limit,2,'validate the effective upstream limit before local filtering');assert.equal(requests.at(-1).query.get('limit'),'5','preserve the caller requested limit');
 const unknown=await api('/search?query=no-headers',{token:u.access_token});assert.equal(unknown.status,200);assert.equal(unknown.data.pagination.has_more,null);assert.equal(unknown.data.pagination.next_page,null);
});
test('real MCP SDK initializes and calls both transports',async()=>{
 const u=await connect();fixtureFor(u,{movies:summaries('movies',251),recent:{movies:events(130)}});for(const kind of ['http','sse']){const client=new Client({name:'integration',version:'1.0.0'});const headers={Authorization:`Bearer ${u.access_token}`};const transport=kind==='http'?new StreamableHTTPClientTransport(new URL(base+'/mcp'),{requestInit:{headers}}):new SSEClientTransport(new URL(base+'/sse'),{requestInit:{headers},eventSourceInit:{fetch:(url,init)=>fetch(url,{...init,headers:{...init?.headers,...headers}})}});
 try{await client.connect(transport);assert.equal((await client.listTools()).tools.length,9);
 const discovered=await client.callTool({name:'trakt_list_operations',arguments:{query:'calendars',limit:10}});assert.equal(discovered.isError,false);assert.ok(JSON.parse(discovered.content[0].text).data.length>0);
 const description=await client.callTool({name:'trakt_get_operation',arguments:{operation_id:'getCalendarsMovies'}});assert.equal(description.isError,false);
 const calendar=await client.callTool({name:'trakt_api_read',arguments:{operation_id:'getCalendarsMovies',path_params:{target:'all',start_date:'2026-09-27',days:7}}});assert.equal(calendar.isError,false,JSON.stringify(calendar));assert.equal(JSON.parse(calendar.content[0].text).data[0].movie.ids.trakt,123);
 const result=await client.callTool({name:'trakt_search',arguments:{query:'A & B'}});assert.equal(result.isError,false);
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
 const next=await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:u.refresh_token}});assert.equal(next.status,200);assert.equal(next.data.scope,'trakt:read');assert.notEqual(next.data.refresh_token,u.refresh_token);
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
 const page=await api('/oauth/authorize?'+params);assert.equal(page.status,200,JSON.stringify(page.data));assert.match(page.data,/make changes you request/);const session_id=page.data.match(/data-session="([^"]+)"/)[1],ticket=page.data.match(/data-ticket="([^"]+)"/)[1];
 assert.equal((await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'unknown'}})).status,400);
 const d=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'start'}});assert.equal(d.status,200);users.get(d.data.device_code).authorized=true;await pause(1100);
 const complete=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'poll'}});assert.equal(complete.status,200);const redirect=new URL(complete.data.redirect);assert.equal(redirect.searchParams.get('state'),'state-123');
 const body={grant_type:'authorization_code',client_id,code:redirect.searchParams.get('code'),redirect_uri:'http://127.0.0.1:9999/callback',code_verifier:verifier,resource};
 assert.equal((await api('/oauth/token',{method:'POST',body:{...body,code_verifier:'x'.repeat(64)}})).status,400);
 assert.equal((await api('/oauth/token',{method:'POST',body:{...body,redirect_uri:'https://evil.test'}})).status,400);
 const tokens=await api('/oauth/token',{method:'POST',body});assert.equal(tokens.status,200);assert.equal(tokens.data.scope,'trakt:read trakt:write');assert.equal((await api('/oauth/token',{method:'POST',body})).status,400);
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

// These fixtures exercise the real OAuth/MCP/Worker boundary against a local HTTP
// server. List writes never reach a live Trakt account.
async function mcpCall(token,name,args={}){
 const response=await api('/mcp',{method:'POST',token,body:{jsonrpc:'2.0',id:501,method:'tools/call',params:{name,arguments:args}}});
 assert.equal(response.status,200,JSON.stringify(response.data));assert.ok(response.data.result,JSON.stringify(response.data));
 return {isError:response.data.result.isError,data:JSON.parse(response.data.result.content[0].text)};
}
async function callSuccess(token,name,args={}){
 const result=await mcpCall(token,name,args);assert.equal(result.isError,false,JSON.stringify(result));return result.data;
}
async function callFailure(token,name,args,error){
 const result=await mcpCall(token,name,args);assert.equal(result.isError,true,JSON.stringify(result));assert.equal(result.data.error,error);return result.data;
}
let browserRegistration;
async function browserConnect(scope){
 browserRegistration??=(await api('/oauth/register',{method:'POST',body:{client_name:'Write consent fixture',redirect_uris:['http://127.0.0.1:9998/callback'],token_endpoint_auth_method:'none'}})).data;
 assert.ok(browserRegistration.client_id,JSON.stringify(browserRegistration));
 const verifier='w'.repeat(64),resource='https://plugin.example.test/',redirect_uri='http://127.0.0.1:9998/callback';
 const params=new URLSearchParams({client_id:browserRegistration.client_id,redirect_uri,response_type:'code',state:'write-fixture',code_challenge:createHash('sha256').update(verifier).digest('base64url'),code_challenge_method:'S256',resource,...(scope?{scope}:{})});
 const page=await api('/oauth/authorize?'+params);assert.equal(page.status,200,JSON.stringify(page.data));
 assert.match(page.data,scope==='trakt:read'?/without making account changes/:/make changes you request/);
 const session_id=page.data.match(/data-session="([^"]+)"/)[1],ticket=page.data.match(/data-ticket="([^"]+)"/)[1];
 const device=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'start'}});assert.equal(device.status,200,JSON.stringify(device.data));
 users.get(device.data.device_code).authorized=true;await pause(1100);
 const complete=await api('/oauth/complete',{method:'POST',body:{session_id,ticket,action:'poll'}});assert.equal(complete.status,200,JSON.stringify(complete.data));
 const redirect=new URL(complete.data.redirect);assert.equal(redirect.searchParams.get('state'),'write-fixture');
 const tokens=await api('/oauth/token',{method:'POST',body:{grant_type:'authorization_code',client_id:browserRegistration.client_id,code:redirect.searchParams.get('code'),redirect_uri,code_verifier:verifier,resource}});
 assert.equal(tokens.status,200,JSON.stringify(tokens.data));assert.equal(tokens.data.scope,scope||'trakt:read trakt:write');
 return {...tokens.data,client_id:browserRegistration.client_id,resource,upstream_user_id:users.get(device.data.device_code).id};
}
let writeAccount,readBrowserAccount;
async function writer(){return writeAccount??=await browserConnect();}
async function readBrowser(){return readBrowserAccount??=await browserConnect('trakt:read');}
const createList=name=>({operation_id:'postUsersListsCreate',path_params:{id:'me'},body:{name,privacy:'private'},confirmed:true});

test('catalog discovery exposes paginated capabilities and exact operation contracts',async()=>{
 const user=await paginationUser(),start=requests.length,ids=new Set();let page=1;
 for(;;){
  const found=await callSuccess(user.access_token,'trakt_list_operations',{page,limit:100});
  assert.equal(found.pagination.item_count,operationCatalog.operations.length);
  for(const row of found.data){assert.equal(ids.has(row.operation_id),false);ids.add(row.operation_id);}
  if(!found.pagination.has_more){assert.equal(found.pagination.next_page,null);break;}page=found.pagination.next_page;
 }
 assert.equal(ids.size,operationCatalog.operations.length);
 const selected=await callSuccess(user.access_token,'trakt_get_operation',{operation_id:'postUsersListsCreate'});
 assert.equal(selected.method,'POST');assert.equal(selected.tool,'trakt_api_write');assert.equal(selected.write_scope_required,true);
 assert.equal(selected.input_schema.properties.body.properties.name.type,'string');
 await callFailure(user.access_token,'trakt_get_operation',{operation_id:'not-real'},'unknown_operation');
 assert.equal(requests.length,start,'catalog discovery must not contact upstream');
});

test('generic reads discover public lists and personal/public release calendars',async()=>{
 const user=await paginationUser(),start=requests.length;
 for(const page of [1,2]){
  const found=await callSuccess(user.access_token,'trakt_api_read',{operation_id:'getListsPopular',query_params:{page,limit:1}});
  assert.equal(found.operation_id,'getListsPopular');assert.equal(found.status,200);assert.equal(found.data[0].list.privacy,'public');
  assert.equal(found.pagination.has_more,page===1);assert.equal(found.pagination.next_page,page===1?2:null);
 }
 for(const target of ['my','all']){
  const calendar=await callSuccess(user.access_token,'trakt_api_read',{operation_id:'getCalendarsMovies',path_params:{target,start_date:'2026-09-27',days:7},query_params:{genres:'drama&token=injected'}});
  assert.equal(calendar.data[0].released,'2026-09-29');assert.equal(calendar.pagination,null,'unpaginated responses have no page contract');
  assert.equal(requests.at(-1).path,`/calendars/${target}/movies/2026-09-27/7`);assert.equal(requests.at(-1).query.get('genres'),'drama&token=injected');assert.equal(requests.at(-1).query.has('token'),false);
 }
 assert.equal(requests.length,start+4,'each requested operation makes one upstream call');
});

test('browser-authorized account creates a private list, adds a movie, reads it back, and deletes it',async()=>{
 const user=await writer(),start=requests.length;
 const created=await callSuccess(user.access_token,'trakt_api_write',createList('Private fixture list'));
 assert.equal(created.status,201);assert.equal(created.data.privacy,'private');const id=String(created.data.ids.trakt);
 assert.equal(requests.at(-1).headers.authorization,`Bearer trakt-${user.upstream_user_id}`);
 const added=await callSuccess(user.access_token,'trakt_api_write',{operation_id:'postUsersListsListAdd',path_params:{id:'me',list_id:id},body:{movies:[{ids:{trakt:123}}]},confirmed:true});
 assert.equal(added.status,201);assert.equal(added.data.added.movies,1);
 const items=await callSuccess(user.access_token,'trakt_api_read',{operation_id:'getUsersListsListItemsAll',path_params:{id:'me',list_id:id}});
 assert.equal(items.data.length,1);assert.equal(items.data[0].movie.ids.trakt,123);assert.equal(items.pagination.has_more,false);
 const deleted=await callSuccess(user.access_token,'trakt_api_write',{operation_id:'deleteUsersListsListDelete',path_params:{id:'me',list_id:id},confirmed:true});
 assert.equal(deleted.status,204);assert.equal(deleted.data,null);assert.equal(mockLists.has(id),false);
 assert.deepEqual(requests.slice(start).map(r=>r.method),['POST','POST','GET','DELETE']);
});

test('write consent, method separation, and strict parameters fail before upstream effects',async()=>{
 const user=await writer(),legacy=await paginationUser(),start=requests.length,valid=createList('Do not create');
 for(const confirmed of [undefined,false]){const args={...valid,confirmed};if(confirmed===undefined)delete args.confirmed;await callFailure(user.access_token,'trakt_api_write',args,'confirmation_required');}
 await callFailure(legacy.access_token,'trakt_api_write',valid,'write_authorization_required');
 await callFailure(user.access_token,'trakt_api_read',valid,'operation_requires_write_tool');
 await callFailure(user.access_token,'trakt_api_write',{operation_id:'getCalendarsMovies',confirmed:true},'operation_requires_read_tool');
 for(const args of [
  {...valid,url:'https://evil.test'}, {...valid,headers:{Authorization:'attacker'}}, {...valid,method:'GET'},
  {...valid,body:{name:'Fixture',unknown:true}}, {...valid,body:{name:123}}, {...valid,path_params:{id:'me',unknown:'x'}},
  {...valid,query_params:{access_token:'attacker'}}, ...['../oauth/token','%2e%2e%2fadmin','https://evil.test','me?x=1','me#fragment','me\nInjected: yes'].map(id=>({...valid,path_params:{id}}))
 ])await callFailure(user.access_token,'trakt_api_write',args,'invalid_api_parameters');
 for(const op of operationCatalog.operations.filter(op=>op.status!=='supported')){
  await callFailure(user.access_token,op.method==='GET'?'trakt_api_read':'trakt_api_write',{operation_id:op.operation_id,confirmed:op.method==='GET'?undefined:true},'operation_unavailable');
 }
 assert.equal(requests.length,start,'rejected input must not touch Trakt');
});

test('explicit read consent survives refresh and cannot be elevated',async()=>{
 const user=await readBrowser(),start=requests.length;
 await callFailure(user.access_token,'trakt_api_write',createList('Forbidden'),'write_authorization_required');
 const elevated=await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:user.refresh_token,client_id:user.client_id,resource:user.resource,scope:'trakt:read trakt:write'}});
 assert.equal(elevated.status,400);assert.equal(elevated.data.error,'invalid_scope');
 const refreshed=await api('/oauth/token',{method:'POST',body:{grant_type:'refresh_token',refresh_token:user.refresh_token,client_id:user.client_id,resource:user.resource}});
 assert.equal(refreshed.status,200,JSON.stringify(refreshed.data));assert.equal(refreshed.data.scope,'trakt:read');
 await callFailure(refreshed.data.access_token,'trakt_api_write',createList('Still forbidden'),'write_authorization_required');
 assert.equal(requests.length,start,'scope rejection and plugin refresh must not call upstream');
});

test('generic API failures preserve safe errors and never retry ambiguous writes',async()=>{
 const user=await writer();
 for(const [status,error,retry] of [[400,'trakt_validation_failed',null],[404,'trakt_not_found',null],[429,'trakt_rate_limited',13],[503,'trakt_upstream_error',null],[200,'invalid_trakt_response',null]]){
  const key='GET /lists/popular';upstreamFailures.set(key,{status,...(status===200?{raw:'<html>private_upstream_detail</html>'}:{})});const start=requests.length;
  try{
   const failed=await callFailure(user.access_token,'trakt_api_read',{operation_id:'getListsPopular'},error);
   assert.equal(failed.retry_after,retry);assert.equal(JSON.stringify(failed).includes('private_upstream_detail'),false);
   assert.equal(requests.length,start+1,'upstream errors are never retried');
  }finally{upstreamFailures.delete(key);}
 }
 const key='POST /users/me/lists',before=mockLists.size,start=requests.length;upstreamFailures.set(key,{status:503,commit:true});
 try{
  await callFailure(user.access_token,'trakt_api_write',createList('Ambiguous fixture'),'trakt_upstream_error');
  assert.equal(requests.length,start+1);assert.equal(mockLists.size,before+1,'fixture committed exactly once before its response failed');
 }finally{upstreamFailures.delete(key);}
 const id=String(nextList);
 const readback=await callSuccess(user.access_token,'trakt_api_read',{operation_id:'getUsersListsListItemsAll',path_params:{id:'me',list_id:id}});
 assert.equal(readback.status,200);assert.deepEqual(readback.data,[]);
 await callSuccess(user.access_token,'trakt_api_write',{operation_id:'deleteUsersListsListDelete',path_params:{id:'me',list_id:id},confirmed:true});
 assert.equal(mockLists.size,before);
});


test('contradictory page counts and truncated final pages cannot claim complete history',async()=>{
 const user=await paginationUser(),rows=summaries('movies',250);
 fixtureFor(user,{movies:rows,headers:{'X-Pagination-Page-Count':'1'}});
 const contradictory=await api('/sync/watched?media_type=movies&page=1&limit=100',{token:user.access_token});
 assert.equal(contradictory.status,502);assert.equal(contradictory.data.error,'invalid_trakt_pagination');
 assert.equal(contradictory.data.pagination,undefined,'contradictory metadata cannot report has_more=false');
 fixtureFor(user,{movies:rows,truncatePage:3,truncatedLength:20});const start=requests.length,partial=[];
 for(const page of [1,2]){
  const result=await api(`/sync/watched?media_type=movies&page=${page}&limit=100`,{token:user.access_token});
  assert.equal(result.status,200);assert.equal(result.data.pagination.has_more,true);partial.push(...result.data.data);
 }
 const truncated=await api('/sync/watched?media_type=movies&page=3&limit=100',{token:user.access_token});
 assert.equal(truncated.status,502);assert.equal(truncated.data.error,'invalid_trakt_pagination');
 assert.equal(truncated.data.data,undefined);assert.deepEqual(partial,rows.slice(0,200));
 assert.equal(watchedRequests(start).length,3,'partial final pages do not trigger retries or erase prior results');
 fixtureFor(user,{movies:rows});
 const resumed=await api('/sync/watched?media_type=movies&page=3&limit=100',{token:user.access_token});
 assert.equal(resumed.status,200);assert.equal(resumed.data.data.length,50);assert.equal(resumed.data.pagination.has_more,false);
 assert.deepEqual([...partial,...resumed.data.data],rows);
});


test('generic API reads reject contradictory completion metadata too',async()=>{
 const user=await paginationUser(),key='GET /lists/popular';
 for(const [page,page_count,length] of [[1,1,100],[3,3,20],[1,3,20],[2,3,20]]){
  upstreamFailures.set(key,{status:200,data:Array.from({length},(_,id)=>({list:{ids:{trakt:id}}})),pagination:{page,page_count,limit:100,item_count:250}});
  const start=requests.length;
  try{
   const error=await callFailure(user.access_token,'trakt_api_read',{operation_id:'getListsPopular',query_params:{page,limit:100}},'invalid_trakt_pagination');
   assert.equal(error.pagination,undefined);assert.equal(requests.length,start+1);
  }finally{upstreamFailures.delete(key);}
 }
});


test('short first or intermediate pages stop traversal before any false complete result',async()=>{
 const user=await paginationUser(),rows=summaries('movies',250);
 for(const badPage of [1,2]){
  fixtureFor(user,{movies:rows,truncatePage:badPage,truncatedLength:20});const partial=[],start=requests.length;
  for(let page=1;page<badPage;page++){
   const prior=await api(`/sync/watched?media_type=movies&page=${page}&limit=100`,{token:user.access_token});
   assert.equal(prior.status,200);partial.push(...prior.data.data);
  }
  const failed=await api(`/sync/watched?media_type=movies&page=${badPage}&limit=100`,{token:user.access_token});
  assert.equal(failed.status,502);assert.equal(failed.data.error,'invalid_trakt_pagination');assert.equal(failed.data.data,undefined);
  assert.equal(watchedRequests(start).length,badPage,'stop at the incomplete page without retrying or fetching later pages');
  assert.deepEqual(partial,rows.slice(0,(badPage-1)*100));
  fixtureFor(user,{movies:rows});
  for(let page=badPage;page<=3;page++){
   const resumed=await api(`/sync/watched?media_type=movies&page=${page}&limit=100`,{token:user.access_token});
   assert.equal(resumed.status,200);assert.equal(resumed.data.pagination.has_more,page<3);partial.push(...resumed.data.data);
  }
  assert.deepEqual(partial,rows);assert.equal(partial.length,250);
 }
});
