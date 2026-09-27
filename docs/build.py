#!/usr/bin/env python3
"""Render the documentation into an empty output directory; no dependencies."""
import argparse
from html import escape
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parent
PAGES = {
    "index": ("Trakt MCP — Your next great watch", "Connect your Trakt account to Claude, Codex, and ChatGPT for viewing history, recommendations, and search."),
    "connect": ("Connect your client — Trakt MCP", "Install Trakt MCP in Claude and Codex, connect ChatGPT web, and approve your own Trakt account with a device code."),
    "reference": ("Technical reference — Trakt MCP", "Trakt MCP tools, HTTP API, OAuth, deployment, isolation, and operational limits."),
}


def build(destination):
    destination = destination.resolve()
    if destination.exists() and any(destination.iterdir()):
        raise SystemExit("Output directory must be empty; choose a fresh directory.")
    destination.mkdir(parents=True, exist_ok=True)
    for name, (title, description) in PAGES.items():
        nav = "".join(
            f'<a href="{page}.html"' + (' aria-current="page"' if page == name else '') + f'>{label}</a>'
            for page, label in [("index", "Overview"), ("connect", "Connect"), ("reference", "Reference")]
        )
        body = (ROOT / "pages" / f"{name}.html").read_text()
        (destination / f"{name}.html").write_text(f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{escape(title)}</title><meta name="description" content="{escape(description, quote=True)}">
<link rel="canonical" href="https://trakt.swacktech.com/{'' if name == 'index' else name + '.html'}">
<meta name="theme-color" content="#12231e"><link rel="stylesheet" href="assets/style.css">
</head><body><a class="skip" href="#main">Skip to content</a>
<header><div class="header-inner"><a class="brand" href="index.html"><span class="brand-mark" aria-hidden="true">t.</span>Trakt MCP</a><nav aria-label="Main navigation">{nav}<a href="https://github.com/swack-tools/trakt-mcp">GitHub ↗</a></nav></div></header>
<main id="main">{body}</main>
<footer><div><a class="brand" href="index.html">Trakt MCP</a><p>Your viewing. Your account. Your conversation.</p></div><p><a href="https://github.com/swack-tools/trakt-mcp">Source on GitHub</a> · <a href="reference.html#original-guide">Original guide</a><br>Community project. Not affiliated with Trakt, Anthropic, or OpenAI.<br>Client guidance checked September 27, 2026.</p></footer>
</body></html>''')
    shutil.copytree(ROOT / "assets", destination / "assets")
    shutil.copytree(ROOT / "archive", destination / "archive")
    (destination / ".nojekyll").touch()
    print(f"Built {len(PAGES)} pages in {destination}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    build(parser.parse_args().output)
