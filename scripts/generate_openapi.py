#!/usr/bin/env python3
"""Generate the checked-in Actions/OpenAPI contract from explicit endpoint schemas."""
import json
from pathlib import Path
S={'type':'string'}
def obj(props,required=()):return {'type':'object','properties':props,'required':list(required),'additionalProperties':False}
def ref(name):return {'$ref':'#/components/schemas/'+name}
def response(schema,description='Success'):return {'description':description,'content':{'application/json':{'schema':schema}}}
error=obj({'error':S,'retry_after':{'type':['integer','null']}},['error'])
media={'type':'object','properties':{'title':S,'year':{'type':['integer','null']},'released':{'type':['string','null']},'first_aired':{'type':['string','null']},'genres':{'type':'array','items':S},'ids':{'type':'object','additionalProperties':True}},'additionalProperties':True}
page=obj({'data':{'type':'array','items':{'type':'object','additionalProperties':True}},'pagination':obj({k:{'type':['integer','null']} for k in ['page','page_count','limit','item_count','next_page']} | {'has_more':{'type':['boolean','null']}}),'filters_applied_to_page':{'type':'boolean'}},['data','pagination'])
device=obj({'device_code':S,'user_code':S,'verification_url':{'type':'string','format':'uri'},'expires_in':{'type':'integer'},'interval':{'type':'integer'},'instructions':S,'session_token':{'type':'string','description':'Temporary plugin bearer credential. This is not a Trakt token. Use only to confirm this login.'}},['device_code','user_code','verification_url','expires_in','interval','instructions'])
token=obj({'access_token':S,'token_type':{'type':'string','const':'Bearer'},'expires_in':{'type':'integer'},'refresh_token':S,'scope':S},['access_token','token_type','expires_in','refresh_token'])
query={
'media_type':{'type':'string','enum':['movie','show','movies','shows','all'],'description':'Movie or show; all is supported by search/history only.'},
'query':{'type':'string','minLength':1,'maxLength':500,'description':'Search text; URL encode reserved characters.'},
'genres':{'type':'string','maxLength':200,'description':'Comma separated lowercase Trakt genre slugs.'},
'years':{'type':'string','pattern':'^[0-9]{4}(-[0-9]{4})?$','description':'Year or inclusive ascending year range.'},
'limit':{'type':'integer','minimum':1,'maximum':100,'default':20},
'page':{'type':'integer','minimum':1,'maximum':4294967295,'default':1}}
query['mode']={'type':'string','enum':['all','recent'],'default':'all'}
query['detail']={'type':'string','enum':['compact','full'],'default':'compact'}
paths={}
def add(path,method,operation,description,schema,params=(),body=None,public=False,status='200'):
 r={status:response(schema),'400':response(ref('Error'),'Invalid request'),'401':response(ref('Error'),'Authorization required'),'429':response(ref('Error'),'Rate limited; observe Retry-After'),'502':response(ref('Error'),'Trakt upstream unavailable or invalid')}
 o={'operationId':operation,'description':description,'responses':r}
 if public:o['security']=[]
 if params:o['parameters']=[{'name':k,'in':'query','required':k=='query','schema':query[k]} for k in params]
 if body:o['requestBody']={'required':True,'content':{'application/json':{'schema':body}}}
 paths.setdefault(path,{})[method]=o
add('/auth/device/code','post','requestDeviceLogin','Start a separate user connection with Trakt device flow. Display user_code and verification_url explicitly. Public calls return a temporary session_token; authenticated calls reconnect the current user. Wait interval seconds before polling.',ref('DeviceLogin'),public=True)
add('/auth/device/token','post','confirmDeviceLogin','Poll once using the temporary session_token as Bearer and the matching device_code. Pending=400, invalid=404, used=409, expired=410, denied=418, slow_down=429. Successful tokens are plugin credentials; Trakt tokens remain private.',ref('PluginTokens'),body=obj({'device_code':S},['device_code']))
add('/sync/watched','get','getWatchedHistory','Read one page, default 100 items. detail=compact keeps IDs, titles, genres, dates and viewing evidence for bounded client payloads; detail=full preserves all upstream metadata. mode=all returns watched summaries; traverse next_page until has_more=false for ALL watched titles before recommending. mode=recent returns watch events newest first, preserving repeat watches; collect the first 100 for recently watched. Traverse movies and shows separately for independent page counts. Missing or failed pages mean incomplete coverage.',{'oneOf':[ref('Page'),obj({'movies':ref('Page'),'shows':ref('Page')},['movies','shows'])]},['media_type','mode','detail','page','limit'])
for parameter in paths['/sync/watched']['get']['parameters']:
 if parameter['name']=='limit': parameter['schema']={**parameter['schema'],'default':100}
add('/recommendations','get','getRecommendations',"Get Trakt's personalized ranking using its viewing/preferences signals. Select movie or show, optionally filter genres and years; 1-100 results. No separate model is trained by this service.",ref('Page'),['media_type','genres','years','limit'])
add('/search','get','searchMedia','Search movies/shows. Genre and year filters apply locally to one returned upstream page. Pagination counts describe the upstream unfiltered result; filters_applied_to_page makes this explicit.',ref('Page'),[k for k in query if k not in ('mode','detail')])
add('/auth/session','delete','deleteConnection','Delete this connection, its stored Trakt tokens, KV cache and open SSE streams. Does not change watch history. Revoke the Trakt app separately in Trakt account settings.',obj({}))
for parameter in paths['/recommendations']['get']['parameters']:
 if parameter['name']=='media_type': parameter['schema']={**parameter['schema'],'enum':['movie','show','movies','shows']}
paths['/auth/session']['delete']['responses']={'204':{'description':'Connection deleted'},'401':response(ref('Error'),'Authorization required')}
add('/oauth/register','post','registerOAuthClient','Register exact HTTPS or loopback callback URIs. Public clients must use S256 PKCE. Confidential OpenAPI Actions clients may request client_secret_post; keep the returned secret private.',{'type':'object','additionalProperties':True},body=obj({'client_name':S,'redirect_uris':{'type':'array','minItems':1,'maxItems':10,'items':{'type':'string','format':'uri'}},'token_endpoint_auth_method':{'type':'string','enum':['none','client_secret_post']}},['redirect_uris']),public=True,status='201')
add('/oauth/token','post','exchangePluginToken','Exchange a single-use authorization code plus verifier, or rotate a plugin refresh token. The optional resource must equal the deployment resource URL advertised by OAuth discovery (including its trailing slash). Client-bound tokens require the original client_id and confidential client secret where applicable.',ref('PluginTokens'),body=obj({k:S for k in ['grant_type','code','code_verifier','redirect_uri','client_id','client_secret','refresh_token','resource']},['grant_type']),public=True)
paths['/oauth/token']['post']['requestBody']['content']['application/x-www-form-urlencoded']=paths['/oauth/token']['post']['requestBody']['content']['application/json']
add('/oauth/authorize','get','authorizeClient','Browser authorization and explicit consent. Public clients require code_challenge_method=S256. Exact registered redirect required.',S,public=True)
paths['/oauth/authorize']['get']['parameters']=[{'name':k,'in':'query','required':k in ['client_id','redirect_uri','response_type'],'schema':S} for k in ['client_id','redirect_uri','response_type','state','scope','resource','code_challenge','code_challenge_method']]
paths['/oauth/authorize']['get']['responses']['200']={'description':'Browser consent page','content':{'text/html':{'schema':S}}}
add('/oauth/complete','post','completeBrowserLogin','Browser-internal device authorization. Requires a short-lived ticket from the consent page.',{'type':'object','additionalProperties':True},body=obj({'session_id':S,'ticket':S,'action':{'type':'string','enum':['start','poll']}},['session_id','ticket','action']),public=True)
rpc={'type':'object','properties':{'jsonrpc':{'type':'string','const':'2.0'},'id':{'type':['string','integer']},'method':S,'params':{'type':'object','additionalProperties':True}},'required':['jsonrpc','method'],'additionalProperties':False}
add('/mcp','post','mcpRequest','MCP JSON-RPC 2.0 over stateless Streamable HTTP. Supports initialize, ping, tools/list and tools/call. Notifications return empty 202. No batches. Supported protocol versions: 2024-11-05, 2025-03-26, 2025-06-18.',{'type':'object','additionalProperties':True},body=rpc)
paths['/mcp']['post']['responses']['202']={'description':'Notification accepted; no body'}
add('/sse','get','openLegacySse','Legacy MCP SSE. First endpoint event gives the POST /messages URL. Send the same Bearer credential on both requests. Maximum eight streams per connection. Reconnect and reinitialize after stream loss.',S)
paths['/sse']['get']['responses']['200']={'description':'Persistent SSE stream','content':{'text/event-stream':{'schema':S}}}
add('/messages','post','legacyMcpMessage','Send JSON-RPC on a live legacy SSE channel. The response is delivered as a message event on that channel.',obj({}),body=rpc)
paths['/messages']['post']['parameters']=[{'name':'session_id','in':'query','required':True,'schema':S}]
paths['/messages']['post']['responses']['202']={'description':'Accepted; response delivered over SSE'}
for path,name in [('/health','health'),('/openapi.json','openApiSchema'),('/.well-known/ai-plugin.json','pluginManifest'),('/.well-known/oauth-authorization-server','authorizationMetadata'),('/.well-known/oauth-protected-resource','resourceMetadata')]:add(path,'get',name,'Public service metadata.',{'type':'object','additionalProperties':True},public=True)
spec={'openapi':'3.1.0','info':{'title':'Trakt MCP API','version':'1.0.0','description':'Multi-user Trakt integration. Each authorization is isolated. Credentials issued by this service are distinct from Trakt access/refresh tokens.'},'servers':[{'url':'https://example.invalid'}],'security':[{'oauth':[]}],'paths':paths,'components':{'securitySchemes':{'oauth':{'type':'oauth2','flows':{'authorizationCode':{'authorizationUrl':'https://example.invalid/oauth/authorize','tokenUrl':'https://example.invalid/oauth/token','scopes':{'trakt:read':'Read personal Trakt data'}}}},'bearerAuth':{'type':'http','scheme':'bearer'}},'schemas':{'Error':error,'Media':media,'Page':page,'DeviceLogin':device,'PluginTokens':token}}}
Path(__file__).resolve().parent.parent.joinpath('openapi.json').write_text(json.dumps(spec,indent=2)+'\n')
