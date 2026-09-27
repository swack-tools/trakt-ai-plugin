# Trakt MCP

Connect your own [Trakt](https://trakt.tv/) account to Claude or Codex for movie and TV recommendations, watched summaries, and title search. Four bundled skills guide useful workflows. Media tools do not change your Trakt history or ratings; login tools change authentication state.

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

## CLI setup

### Claude Code

```sh
claude plugin marketplace add swack-tools/trakt-ai-plugin
claude plugin install trakt-mcp@trakt-mcp
claude
```

In the new session, open `/mcp`, select the Trakt server, and authenticate.

### Codex CLI

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

## Four bundled workflows

- **what-to-watch** — a concise shortlist grounded in Trakt recommendations.
- **watching-profile** — summarize returned watched titles and play counts, without inventing a dated timeline.
- **find-title** — disambiguate titles, years, and remakes with bounded search.
- **connection-help** — diagnose installation, OAuth, device codes, and rate limits.

Use natural language or your client’s skill picker. See the [plugin README](plugins/trakt-mcp/README.md) for setup, workflow details, and platform limits.

## Try asking

- “Recommend five movies for me using Trakt.”
- “What TV shows does Trakt recommend for me?”
- “Recommend science-fiction movies from 2020 onward.”
- “Show me the movies and TV shows I’ve watched.”
- “Search Trakt for Arrival.”

Recommendations come from Trakt’s personalized results. History shows watched titles and play counts.

For standalone MCP or skills-only setup, see the [installation guide](https://trakt.swacktech.com/connect.html). Skills alone still require the authenticated MCP connection.

This is a community plugin, not an approved directory listing or an official Trakt integration. Directory eligibility, publisher verification, and reviewer access remain owner responsibilities.

See the [full documentation](https://trakt.swacktech.com/) for other clients, troubleshooting, API details, and self-hosting.

Licensed under [GPL-3.0-only](LICENSE).
