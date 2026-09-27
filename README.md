# Trakt MCP

Connect your own [Trakt](https://trakt.tv/) account to Claude or Codex for movie and TV recommendations, watched history, and search. Read-only: it does not change your Trakt history or ratings.

## Desktop setup

### Claude

1. Open **Customize → Plugins → Add → Add marketplace → Add from a repository**.
2. Enter `swack-tools/trakt-mcp` and add the marketplace.
3. Install **Trakt MCP**, then start a new chat and follow the connection prompt.

### Codex

Add the marketplace once from a terminal:

```sh
codex plugin marketplace add swack-tools/trakt-mcp
```

Restart the Codex app, open **Plugins**, choose the **Trakt MCP** marketplace, and install **Trakt MCP**. Start a new chat and follow the connection prompt.

## CLI setup

### Claude Code

```sh
claude plugin marketplace add swack-tools/trakt-mcp
claude plugin install trakt-mcp@trakt-mcp
claude
```

In the new session, open `/mcp`, select the Trakt server, and authenticate.

### Codex CLI

```sh
codex plugin marketplace add swack-tools/trakt-mcp
codex plugin add trakt-mcp@trakt-mcp
codex
```

Follow the connection prompt when you first use Trakt.

## Connect your Trakt account

Each person signs in with their own Trakt account:

1. On the authorization page, choose **Connect Trakt** to display a device code.
2. Open [trakt.tv/activate](https://trakt.tv/activate), sign in, enter the code, and approve access.
3. Return to the authorization page; it finishes connecting automatically. Then return to your chat.

## Try asking

- “Recommend five movies for me using Trakt.”
- “What TV shows does Trakt recommend for me?”
- “Recommend science-fiction movies from 2020 onward.”
- “Show me the movies and TV shows I’ve watched.”
- “Search Trakt for Arrival.”

Recommendations come from Trakt’s personalized results. History shows watched titles and play counts.

See the [full documentation](https://trakt.swacktech.com/) for other clients, troubleshooting, API details, and self-hosting.

Licensed under [GPL-3.0-only](LICENSE).
