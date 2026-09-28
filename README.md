# Trakt MCP

Connect your own [Trakt](https://trakt.tv/) account to Claude or Codex for recommendations, viewing history, title search, lists, and release calendars. Version 2.0.0 adds a documented API catalog and requested changes to your watchlist, history, ratings, and collection. Seven bundled skills guide these workflows; writes require explicit intent and a connection with write permission.

## Desktop setup

### Claude

1. Open **Customize → Plugins → Add → Add marketplace → Add from a repository**.
2. Enter `swack-tools/trakt-ai-plugin` and add the marketplace.
3. Install **Trakt MCP**, then start a new chat and follow the connection prompt.

### Codex

Add the marketplace once from a terminal:

```sh
codex plugin marketplace add swack-tools/trakt-ai-plugin
```

Restart the Codex app, open **Plugins**, choose the **Trakt MCP** marketplace, and install **Trakt MCP**. Start a new chat and follow the connection prompt.

## Command-line setup

### Claude Code

```sh
claude plugin marketplace add swack-tools/trakt-ai-plugin
claude plugin install trakt-mcp@trakt-mcp
claude
```

In the new session, open `/mcp`, select the Trakt server, and authenticate.

### Codex command-line tool

```sh
codex plugin marketplace add swack-tools/trakt-ai-plugin
codex plugin add trakt-mcp@trakt-mcp
codex
```

Follow the connection prompt when you first use Trakt.

## Connect your Trakt account

Each person signs in with their own Trakt account:

1. On the authorization page, choose **Connect Trakt** to display a device code.
2. Open [trakt.tv/activate](https://trakt.tv/activate), sign in, enter the code, and approve access.
3. Return to the authorization page; it finishes connecting automatically. Then return to your chat.

## Seven bundled workflows

- **what-to-watch**: a concise shortlist grounded in Trakt recommendations.
- **watching-profile**: retrieve every watched-summary page, or summarize the latest 100 watch events per medium.
- **find-title**: disambiguate titles with bounded search, or follow every page when you explicitly request all results.
- **lists-and-watchlist**: discover public lists, create personal lists, and make requested item changes.
- **upcoming-releases**: read personal or public calendars for a bounded date range and time zone.
- **manage-library**: record or remove specific watches, ratings, and collection items.
- **connection-help**: diagnose installation, OAuth, device codes, and rate limits.

Use natural language or your client’s skill picker. See the [plugin README](plugins/trakt-mcp/README.md) for setup, workflow details, and platform limits.

## API coverage and write access

Nine MCP tools retain the five focused history, recommendation, search, and login tools and add `trakt_list_operations`, `trakt_get_operation`, `trakt_api_read`, and `trakt_api_write`. The catalog represents the official API operations with exact parameter schemas and availability status. It includes managed authentication and restricted first-party operations; catalog coverage does not mean every operation is callable. Discover and inspect an operation before using it. Read and write tools accept no raw URLs, headers, or tokens. See the [API reference](https://trakt.swacktech.com/reference.html#api-coverage).

New browser OAuth connections request read and write access when scope is omitted, and the consent page states those permissions. Explicit `trakt:read` requests stay read-only. Existing tokens and standalone device login remain read-only; reauthorize through your client with `trakt:read trakt:write` for writes. Refresh cannot increase scope. The write tool also requires `confirmed: true` for a concrete change the user requested. Skills verify changes through reads and never automatically repeat a write after an uncertain failure.

## Try asking

- `Recommend five movies for me using Trakt.`
- `What TV shows does Trakt recommend for me?`
- `Recommend science-fiction movies from 2020 onward.`
- `Show me the movies and TV shows I’ve watched.`
- `Search Trakt for Arrival.`
- `Create a private list called Rainy Sunday and add Arrival (2016).`
- `Show my upcoming episodes next week in America/Chicago.`
- `Rate Arrival (2016) 8.`

Recommendations come from Trakt’s personalized results. By default, history returns watched-title summaries with play counts, one compact page per tool call. Compact detail keeps identifying metadata, genres, counts, and watch timestamps; request full detail only for additional metadata. The skills follow `pagination.next_page` to completion for all-history requests and recommendations based on all viewing. Ask for `recently watched` to use the latest 100 chronological watch events per medium, including repeat watches. Interrupted traversals are reported as partial with a resume page.

For standalone MCP or skills-only setup, see the [installation guide](https://trakt.swacktech.com/connect.html). Skills alone still require the authenticated MCP connection.

This community plugin is independent of Trakt. Repository installation does not establish approval by a public directory. Directory eligibility, publisher verification, and reviewer access remain owner responsibilities.

See the [full documentation](https://trakt.swacktech.com/) for other clients, troubleshooting, API details, and self-hosting.

Licensed under [GPL-3.0-only](LICENSE).
