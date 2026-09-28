# Trakt MCP community plugin

![Community cinema card: a cream play card and gold discovery star on green](assets/mark.svg)

An independent community plugin for choosing films and television with your own Trakt account. Seven bundled skills support recommendations, viewing profiles, title search, list management, release calendars, library changes, and connection diagnosis. This project is not endorsed by Trakt, Anthropic, or OpenAI. Installing from the repository does not mean any platform has approved a directory listing.

## Install the plugin

The public repository is [swack-tools/trakt-ai-plugin](https://github.com/swack-tools/trakt-ai-plugin). Its marketplace and plugin identifiers remain `trakt-mcp`.

**Claude desktop chat, `claude.ai`, and Cowork:** open **Customize → Plugins → Add → Add marketplace**, enter the repository URL, and install `Trakt MCP — Community`. Alternatively, use **Add → Upload plugin** with the packaged plugin ZIP. Open the installed plugin's **Connectors** tab and connect the Trakt server. A Team or Enterprise organization may require its Owner to add the connector first. A command-line install is local to that machine and does not automatically add the plugin to your Claude account.

**Claude Code:** run these commands, then open `/mcp` in Claude Code and complete the connection flow when prompted:

```sh
claude plugin marketplace add swack-tools/trakt-ai-plugin
claude plugin install trakt-mcp@trakt-mcp
```

**Codex app and command-line tool:** add the repository marketplace, then install the plugin:

```sh
codex plugin marketplace add swack-tools/trakt-ai-plugin
codex plugin add trakt-mcp@trakt-mcp
```

In the desktop app, open the Plugins directory, select the repository marketplace, and check the installed plugin's connection. Restart the app or open a new chat after updating installed files. Client commands and UI availability depend on version and workspace policy; use your client's MCP login control if the plugin prompts for authorization. A local marketplace install is separate from a ChatGPT workspace or public directory publication.

During browser authorization, approve the service connection, open the displayed Trakt activation page, enter the device code, and return to finish. Each person authorizes their own Trakt account. You do not need the operator's API key or Cloudflare credentials. Do not paste access tokens, passwords, or the private device code into chat or a support issue.

## Use the seven skills

| Skill | Try asking | What it actually does |
| --- | --- | --- |
| **What to watch** | `Suggest three science-fiction movies I haven't watched.` | Gets Trakt recommendations; traverses every watched page for all-history grounding or unwatched filtering, or uses the latest 100 events for recent viewing. |
| **Watching profile** | `Which genres appear most in my watched shows?` | Traverses all watched summaries by default, or summarizes the latest 100 events per medium on a recent-viewing request. |
| **Find title** | `Help me distinguish the different versions of The Thing.` | Searches title/year/type, asks about ambiguity, and supplies returned identifiers. |
| **Lists and watchlist** | `Create a private list called Rainy Sunday and add Arrival (2016).` | Discovers public lists and performs specifically requested changes to owned lists or the watchlist. |
| **Upcoming releases** | `What episodes air next week in America/Chicago?` | Reads personal or public calendars for bounded dates and explains time zones and release types. |
| **Manage library** | `Rate Arrival (2016) 8 and add it to my collection.` | Resolves titles, executes the two requested effects, and reads back each result. |
| **Connection help** | `My Trakt authorization expired; help me reconnect.` | Separates installation, client OAuth, device login, and upstream failures. |

Claude Code exposes skills under names such as `/trakt-mcp:what-to-watch`; other clients may select them from natural language or use their own skill picker. The instructions resolve the actual installed tool names rather than assuming a client namespace.

The focused read tools are `trakt_search`, `trakt_get_watched_history`, and `trakt_get_recommendations`. The login tools, `trakt_request_login` and `trakt_confirm_login`, change authentication state. Four additional tools expose the documented API through operation IDs:

| Tool | Purpose |
| --- | --- |
| `trakt_list_operations` | Search or filter the local operation catalog by query/category, one catalog page at a time. |
| `trakt_get_operation` | Inspect exact path, query, and body schemas, authentication, availability, restrictions, and write annotations. |
| `trakt_api_read` | Execute one supported GET operation with schema-validated parameters. |
| `trakt_api_write` | Execute one supported write with explicit user intent, `confirmed: true`, and `trakt:write` permission. |

Catalog coverage represents documented operations, including managed authentication and unavailable first-party operations. Presence in the catalog does not guarantee access through this integration or eligibility for an account feature. Inspect an operation before calling it; never invent IDs or parameters. Generic tools accept no raw URLs, headers, or tokens. Release or streaming metadata, where available, does not grant playback rights or prove access in a particular subscription.

New browser OAuth connections request `trakt:read trakt:write` when scope is omitted; the consent page describes writes. Explicit read-only requests remain read-only. Existing tokens and standalone device login retain read-only scope. Reauthorize through the client's OAuth flow for writes; refresh cannot elevate permission. A requested recommendation never authorizes saving or rating it. Skills execute a clear, concrete change without asking for the same confirmation twice, inspect per-item results, and read back the state. After an ambiguous failure, they check state instead of automatically replaying a write.

Generic responses contain `operation_id`, upstream `status`, `data` (object, array, or empty result), and `pagination`. Paginated reads fetch one upstream page; unknown continuation metadata remains unknown and does not establish complete retrieval. Account restrictions, upstream validation, and rate limits still apply. See the [catalog reference](https://trakt.swacktech.com/reference.html#api-coverage) and the source snapshot under `api/trakt/` in the repository.

`trakt_get_watched_history` accepts `media_type: movie|show|all`, `mode: all|recent` (default `all`), `page` (default 1), `limit` (1–100, default 100), and `detail: compact|full` (default `compact`). Each call returns one upstream page per requested medium. Compact detail reduces response size for traversal; full detail preserves the original upstream metadata for specific needs. `mode: all` returns watched-title summaries and play counts; `mode: recent` returns chronological watch events, newest first, including repeated watches. A show summary need not mean you finished the series; show events can represent episodes.

Compact rows retain supplied `plays`, `last_watched_at`, `last_updated_at`, `total_count`, history `id`, `watched_at`, `action`, and `type`. Movie/show metadata retains `ids`, `title`, `year`, `genres`, `released`, `first_aired`, and `runtime`; episode metadata retains `ids`, `title`, `season`, and `number`. Fields absent upstream remain absent. Search and recommendations do not accept `detail`.

Responses contain `data` and `pagination` (`page`, effective `limit`, `page_count`, `item_count`, `has_more`, `next_page`). For `media_type: all`, these are separate under `movies` and `shows`. The skills explicitly request `detail: compact` for both all-history and recent traversal and follow each medium’s `next_page` sequentially until `has_more` is false and `next_page` is null for all-history requests and recommendations grounded in all viewing. They maintain IDs and counts and report progress; no arbitrary page budget truncates an explicit all-history request. Recent-viewing workflows gather only the first 100 events per medium, fetching more pages if Trakt clamps the page size. Each continuation keeps the same requested limit to preserve page offsets.

Search also returns normalized pagination. Ordinary title lookup inspects at most three pages; an explicit request for all search results follows `next_page` to completion (`has_more: false` and `next_page: null`). Unknown continuation metadata means partial coverage, even when no next page is supplied. Genre/year filters apply to each upstream page, so pagination counts describe the unfiltered query and an empty filtered page may still have more results. Recommendations accept `limit` up to 100, but no `page` parameter. They come from Trakt; the assistant explains their fit without claiming to know why Trakt ranked each result.

## Plugin, MCP alone, or skills alone

The plugin combines portable instructions with a remote server. It runs no installation scripts, hooks, downloaded programs, or local MCP processes. No separate runtime or API key is required for normal hosted use.

For **MCP only**, add the hosted deployment URL `https://trakt.swacktech.com/mcp` as a remote Streamable HTTP server in your client and complete its OAuth flow. Claude Code supports `claude mcp add --transport http trakt https://trakt.swacktech.com/mcp`; Codex supports `codex mcp add trakt --url https://trakt.swacktech.com/mcp` followed by `codex mcp login trakt`. Do not install a second connection to the same server if the plugin already supplies it. Legacy SSE at `/sse` is available for compatible clients, but Streamable HTTP is preferred. MCP-only setup gives tools without these workflow instructions.

For **skills only**, copy individual folders from this package's `skills` directory to the appropriate client skill location, preserving each folder's contents. Claude Code project skills live in `.claude/skills`; Codex project skills live in `.agents/skills`. For Claude chat/desktop, upload each skill ZIP in **Customize → Skills**, then turn it on. Each skill is self-contained, including its OpenAI dependency metadata. It still requires an authorized Trakt MCP connection; uploading instructions does not register a server in a directory. Do not copy the same skills separately when the plugin already loads them.

The fixed server URLs in this package describe this project's hosted deployment. A self-hosted distributor must replace the URL in both MCP configuration files and in each skill's OpenAI dependency metadata, and supply its own service documentation. Portable `mcp.json` uses `streamable-http`; Claude/legacy `.mcp.json` uses `http`. Their different type spellings are intentional.

## Support by surface

| Surface | Skills and remote MCP | Important limitation |
| --- | --- | --- |
| Claude chat on web, desktop, mobile | Supported component types | Connect the remote server from the plugin's Connectors tab; plan and administrator rules apply. |
| Claude Cowork | Supported component types | Connect the bundled server; account installation and policy govern access. |
| Claude Code | Skills and remote MCP | Local installs are per machine; authenticate through its MCP controls. |
| Codex app / command-line tool | Local marketplace package, skills, remote MCP | Version and workspace policy govern availability; test a new session after updates. |
| OpenAI shared ChatGPT/Codex directory | Proposed `With MCP` server plus skills upload | Owner identity, eligibility, verification, review, and publication are separate; this package is not a directory approval. |

No Claude-only agent is bundled: these workflows are short enough to keep their complete procedure in portable skills. An `agents` directory would not give the same behavior on all clients; Claude chat ignores Claude agent files. Per-skill `agents/openai.yaml` files here are skill metadata and MCP dependencies, not autonomous agents. There is no MCP UI or carousel screenshot requirement represented by the artwork.

## Data and privacy

The client sends tool arguments, such as queries, filters, identifiers, requested write bodies, or a login confirmation code, to the hosted service on Cloudflare. The service forwards the selected reads or writes to Trakt and returns metadata, account records, operation results, or connection status. Requested writes change the connected Trakt account; this service does not keep a separate local history database. Returned data becomes available to the assistant in your chosen client and is subject to that provider's policies. Skills do not send the entire conversation or inspect private local files.

The service stores authorization and connection state in Cloudflare Durable Objects, with a KV token mirror. Trakt access and refresh tokens stay on the server rather than being returned by media tools. A login tool returns activation details and a private device code needed to complete that connection. Token expiration is not a promise of deletion. Removing the plugin from a client does not prove that server state was deleted or that Trakt access was revoked. Consult the service's [privacy information](https://trakt.swacktech.com/privacy) and [deployment documentation](https://trakt.swacktech.com/reference.html) for session deletion and revoke the integration through Trakt's account controls when appropriate. Cloudflare and the client provider may process request metadata independently of the app.

## Troubleshooting and support

If skills appear but tools do not, enable or connect the bundled connector and start a fresh conversation. If an MCP request is unauthorized before a tool runs, use the client's OAuth connection flow. If a callable tool reports `trakt_login_required`, ask to reconnect your Trakt account. Wait for the stated polling interval; an expired device code must be replaced, not retried indefinitely. Rate limits and service outages are not fixed by repeated login attempts.

An empty recommendation list can reflect your Trakt account or filters. An empty filtered search page does not establish that no matching title exists. If a page fails, is truncated, or reports inconsistent pagination, the assistant reports partial coverage. It includes the next page to resume. It respects retry delays and never claims to have used all viewing while pages are missing. Large pages can exceed response limits; retry a smaller page size from page 1 to avoid changing offsets midway, and deduplicate summary IDs. A recent 100-event sample does not establish complete calendar-period statistics.

For reproducible bugs, use [GitHub issues](https://github.com/swack-tools/trakt-ai-plugin/issues) and include the client/version, workflow, and redacted error code. Never post credentials, device codes, or personal watched history. The [documentation site](https://trakt.swacktech.com/) contains the operator and installation guides. Publisher identity, legal terms, private reviewer access, and directory permissions remain owner responsibilities.

## License and assets

This package is licensed under [GPL-3.0-only](LICENSE). Its original geometric artwork is also covered by that license; [asset provenance](assets/README.md) records the source and limitations. Trakt and platform names identify interoperable services, not project ownership of their marks. Version 2.0.0 adds an API operation catalog, generic read and write tools, explicit write authorization, and three additional skills. It preserves the five focused tools, compact watched responses, full-history traversal, and the `trakt-mcp` identity.

## Publisher and private reports

Publisher: SwackTech LLC (owner-supplied identity; platform verification is a separate submission step). Ordinary support: [repository issues](https://github.com/swack-tools/trakt-ai-plugin/issues). Security reports: [security@swacktech.com](mailto:security@swacktech.com). Do not post credentials, device codes, private viewing data, or vulnerability details in public issues. The contact was supplied by the owner; inbox delivery and response times were not tested.
