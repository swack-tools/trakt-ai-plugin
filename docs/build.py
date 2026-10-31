#!/usr/bin/env python3
"""Render the documentation into an empty output directory; no dependencies."""
import argparse
from html import escape
from pathlib import Path
import shutil
import os
from urllib.parse import urlsplit

# Public documentation deployment, independent of Worker configuration.
BASE_URL = os.environ.get("DOCS_BASE_URL", "https://trakt.swacktech.com").rstrip("/")
if (urlsplit(BASE_URL).scheme != "https" or not urlsplit(BASE_URL).hostname
        or urlsplit(BASE_URL).path or urlsplit(BASE_URL).query or urlsplit(BASE_URL).fragment
        or urlsplit(BASE_URL).username or urlsplit(BASE_URL).password):
    raise SystemExit("DOCS_BASE_URL must be an HTTPS origin")

ROOT = Path(__file__).resolve().parent
PAGES = {
    "index": ("Companion for Trakt — Your next great watch", "Connect your Trakt account to Claude, Codex, and ChatGPT for viewing history, recommendations, and search."),
    "connect": ("Connect your client — Companion for Trakt", "Install Companion for Trakt in Claude and Codex, connect ChatGPT web, and approve your own Trakt account."),
    "skills": ("Four useful workflows — Companion for Trakt", "Choose a movie, understand watched summaries, find a title, or diagnose your connection."),
    "data-privacy": ("Privacy and data handling — Companion for Trakt", "What the hosted Companion for Trakt service stores, sends, and deletes."),
    "terms": ("Software and service terms — Companion for Trakt", "Software license, service limitations, and operator responsibilities."),
    "support": ("Support and security — Companion for Trakt", "Get help safely and report problems with Companion for Trakt."),
    "reference": ("Technical reference — Companion for Trakt", "Companion for Trakt tools, HTTP API, OAuth, deployment, isolation, and operational limits."),
}


def build(destination):
    destination = destination.resolve()
    if destination.exists() and any(destination.iterdir()):
        raise SystemExit("Output directory must be empty; choose a fresh directory.")
    destination.mkdir(parents=True, exist_ok=True)
    for name, (title, description) in PAGES.items():
        nav = "".join(
            f'<a href="{page}.html"' + (' aria-current="page"' if page == name else '') + f'>{label}</a>'
            for page, label in [("index", "Overview"), ("connect", "Install"), ("skills", "Workflows"), ("reference", "Developers")]
        )
        body = (ROOT / "pages" / f"{name}.html").read_text()
        (destination / f"{name}.html").write_text(f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{escape(title)}</title><meta name="description" content="{escape(description, quote=True)}">
<link rel="canonical" href="{escape(BASE_URL, quote=True)}/{'' if name == 'index' else name + '.html'}">
<meta name="theme-color" content="#12231e"><link rel="stylesheet" href="assets/style.css">
</head><body><a class="skip" href="#main">Skip to content</a>
<header><div class="header-inner"><a class="brand" href="index.html"><span class="brand-mark" aria-hidden="true">c.</span>Companion for Trakt</a><nav aria-label="Main navigation">{nav}<a href="https://github.com/swack-tools/trakt-ai-plugin">GitHub ↗</a></nav></div></header>
<main id="main">{body}</main>
<footer><div><a class="brand" href="index.html">Companion for Trakt</a><p>Your viewing. Your account. Your conversation.</p></div><p><a href="https://github.com/swack-tools/trakt-ai-plugin">Source on GitHub</a> · <a href="data-privacy.html">Privacy</a> · <a href="terms.html">Terms</a> · <a href="support.html">Support &amp; security</a><br>Companion for Trakt is an independent, unofficial integration by SwackTech LLC. Companion for Trakt uses Trakt but is not endorsed or certified by Trakt. Not affiliated with Anthropic or OpenAI.<br>Client guidance checked September 27, 2026.</p></footer>
</body></html>''')
    shutil.copytree(ROOT / "assets", destination / "assets")
    (destination / ".nojekyll").touch()
    print(f"Built {len(PAGES)} pages in {destination}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    build(parser.parse_args().output)
