# Trakt MCP

A multi-user Rust/WASM Cloudflare Worker at **https://trakt.swacktech.com**. Each person connects their own Trakt account. No shared Trakt login or operator credentials are required by users.

## Connect a client

For Claude Code's modern HTTP transport:

```sh
claude mcp add --transport http trakt https://trakt.swacktech.com/mcp
```

Open `/mcp` in Claude Code and authenticate. The browser shows which client is requesting access. Choose **Connect Trakt**, open **https://trakt.tv/activate**, enter the displayed code, and approve in Trakt. Return to the browser page; it completes authorization automatically.

For legacy clients, use **https://trakt.swacktech.com/sse**. The `.claude-plugin` manifest explicitly points to its SSE configuration. For local plugin testing, clone this repository and run `claude --plugin-dir .`. The repository is private, so GitHub installation requires repository access; the deployed remote service can still be used by anyone with a Trakt account.

Codex and other OAuth-capable MCP clients can use **https://trakt.swacktech.com/mcp**. Configure it as a remote HTTP MCP server and use that client's login flow. OAuth discovery, dynamic client registration, S256 PKCE, authorization-code exchange and rotating refresh tokens are provided. Public clients must support PKCE.

Available tools:

| Tool | Purpose |
|---|---|
| `trakt_request_login` | Reconnect the current user's Trakt account using a device code |
| `trakt_confirm_login` | Confirm the matching device code after approval |
| `trakt_get_watched_history` | Watched movies/shows with full metadata |
| `trakt_get_recommendations` | Trakt-personalized movie/show results with genre/year filters |
| `trakt_search` | Movie/show search, optional genre/year filtering and pagination |

Recommendations preserve Trakt's own ranking and preference signals. The service does not train an additional ratings model. Watched history is the `/sync/watched` summary with play counts, not an individual play-event feed. Search filters apply to the fetched upstream page; `filters_applied_to_page` and original pagination counts make this explicit.

## OpenAPI Actions and HTTP

Import **https://trakt.swacktech.com/openapi.json**. The API supports OAuth authorization code and `client_secret_post` for confidential Actions clients:

1. Obtain the exact callback URL displayed by the Actions client.
2. POST `/oauth/register` with `client_name`, `redirect_uris: ["<exact callback>"]`, and `token_endpoint_auth_method: "client_secret_post"`.
3. Save the returned `client_id` and `client_secret` in the Actions client's OAuth configuration. Never put the secret in the schema or chat.
4. Set authorization URL to `/oauth/authorize`, token URL to `/oauth/token`, scope to `trakt:read`, and token exchange to POST body credentials.

Public OAuth clients use `token_endpoint_auth_method: "none"` and S256 PKCE. Redirect URIs must be exact registered HTTPS URLs or local loopback HTTP URLs. `https://trakt.swacktech.com/` is the common OAuth resource identifier for HTTP and SSE. This is an OAuth/API integration; importing the schema alone does not register or publish an app in any marketplace.

For an interactive command-line login, run `python3 scripts/login.py`; it displays the activation code and saves plugin credentials privately under your user configuration directory.

Direct HTTP clients can POST `{}` to `/auth/device/code`. Save the returned `session_token` privately and display `user_code`, `verification_url` and `instructions`. After at least `interval` seconds, POST `{"device_code":"..."}` to `/auth/device/token` with `Authorization: Bearer <session_token>`. On success, replace the temporary token with the returned plugin `access_token` and securely save `refresh_token`. These are **plugin** credentials, never Trakt credentials. Plugin access tokens last one hour; refresh tokens expire after 30 days and rotate on every use. Reusing an already-consumed refresh token revokes that connection; reauthorize after 1,024 rotations. Exchange a direct-login refresh token at `/oauth/token` with `grant_type=refresh_token`; no client ID is needed for direct login. OAuth client-bound refreshes require the original client ID and, for confidential clients, client secret.

Authenticated data endpoints:

```text
GET /sync/watched?media_type=all
GET /recommendations?media_type=movie&genres=science-fiction&years=2020-2026&limit=10
GET /search?query=Arrival&media_type=movie&page=1&limit=20
DELETE /auth/session
```

Device polling statuses: 400 pending, 404 invalid code, 409 already used, 410 expired, 418 denied, 429 slow down. Respect `Retry-After`; do not retry non-idempotent token exchanges blindly. Delete `/auth/session` to erase this connection and stored tokens. Revoke the app in Trakt settings to revoke Trakt-side authorization too.

## Deployment

Prerequisites: Node 22+, rustup stable with `wasm32-unknown-unknown`, GitHub CLI and Wrangler authentication, SSH/GPG commit signing, and the `.env.example` variables in ignored `.env`. Configure the Trakt application to allow device authentication. `TRAKT_REDIRECT_URI` must match its configured redirect for refresh; the default is `urn:ietf:wg:oauth:2.0:oob`.

```sh
rustup target add wasm32-unknown-unknown
cargo install worker-build --version 0.1.14 --locked
npm ci
python3 scripts/manage.py configure
python3 scripts/manage.py sync-secrets
cargo fmt --check
cargo test
cargo clippy --target wasm32-unknown-unknown -- -D warnings
npm run build
npm run test:integration
npx wrangler deploy --dry-run
python3 scripts/manage.py deploy
python3 scripts/smoke.py
```

Use rustup's `cargo` and `rustc` consistently. If Homebrew Rust is ahead of rustup in PATH, run build commands with `PATH="$HOME/.cargo/bin:$PATH"`. worker-build 0.1.14 is the build-tool line compatible with the requested workers-rs 0.5; the later 0.7+ builders reject that SDK.

Pushes to `main` run the same validation and deploy with encrypted Worker secrets. Pull requests validate without deploying. The supplied KV namespace is bound as `TRAKT_SESSIONS`. The deployment creates SQLite Durable Objects through migration `v1`, and attaches the custom domain. `workers.dev` and preview URLs are disabled. `.env`, artifacts and Cargo.lock follow the requested ignore contract; npm is locked, but Cargo resolves the requested compatible dependency ranges afresh in CI.

## Isolation and operational limits

- Each authorization creates a separate Durable Object with hashed plugin credentials, encrypted-at-rest Trakt credentials, and a per-connection KV cache. Durable storage is authoritative; KV is never used to refresh stale credentials. An async mutex serializes token rotation within each connection.
- No tokens or request bodies are logged by application code. Responses use `no-store`; browser login has a restrictive CSP and no third-party scripts. OAuth codes expire after two minutes and are single use. Browser login tickets expire after fifteen minutes.
- Browser origins are allowlisted for this domain, ChatGPT and Claude. Native clients send no Origin. Authentication is enforced independently of CORS. Public registration/login starts are limited to 20/minute per source IP; authenticated requests are limited to 120/minute per connection. Temporary login credentials cannot access data tools. Deployments intended for heavier public traffic should tune abuse and cost limits for their needs.
- Request bodies are capped at 64 KiB, upstream responses at 8 MiB, and upstream requests at 20 seconds. Oversized responses fail explicitly. Full JSON processing allocates memory; this service does not claim zero-allocation serialization.
- Streamable HTTP returns JSON and accepts notifications with 202. GET `/mcp` returns 405. Legacy SSE supports up to eight streams per connection with bounded queues and a 1 MiB event limit; transport state is ephemeral, so reconnect and initialize after disconnection. It does not claim event replay.
- Integration tests use fake Trakt accounts in local workerd. They do not modify real viewing history. Deployment checks establish infrastructure behavior; a human must approve Trakt login to verify live personal history/recommendations.

## API references

Implementation contracts were checked against [Trakt device flow](https://docs.trakt.tv/reference/postoauthdevicetoken.md), [single-use refresh tokens](https://docs.trakt.tv/reference/postoauthtoken.md), [recommendation filters](https://docs.trakt.tv/reference/getrecommendationsmoviesrecommend.md), [MCP transports](https://modelcontextprotocol.io/specification/2025-03-26/basic/transports) and [Cloudflare secrets](https://developers.cloudflare.com/workers/configuration/secrets/).
