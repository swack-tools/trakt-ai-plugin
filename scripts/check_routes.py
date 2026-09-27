#!/usr/bin/env python3
"""Keep every public Worker handler routed while GitHub Pages serves the docs."""
from pathlib import Path
import re
import tomllib

ROOT = Path(__file__).resolve().parent.parent
HOST = "trakt.swacktech.com"


def check():
    config = tomllib.loads((ROOT / "wrangler.toml.example").read_text())
    routes = config["routes"]
    prefixes = []
    for route in routes:
        assert not route.get("custom_domain"), "A Worker Custom Domain captures the documentation"
        assert route["zone_name"] == "swacktech.com"
        pattern = route["pattern"]
        assert pattern.startswith(HOST + "/") and pattern.endswith("*")
        assert pattern.count("*") == 1, "Only trailing wildcards are supported"
        prefixes.append(pattern[len(HOST):-1])

    def worker_handles(path):
        return any(path.startswith(prefix) for prefix in prefixes)

    source = (ROOT / "src/http.rs").read_text().split("pub async fn route(", 1)[1]
    source = source.split("pub async fn bounded(", 1)[0]
    paths = set(re.findall(r'"(/[a-zA-Z0-9_./-]*)"', source))
    # The original root landing page is replaced by Pages; /_... are DO RPCs.
    paths = {path for path in paths if path != "/" and not path.startswith("/_")}
    assert "/mcp" in paths and "/oauth/authorize" in paths, "Handler extraction failed"
    for path in sorted(paths):
        for suffix in ("", "?query=test&page=2"):
            assert worker_handles(path + suffix), f"Public handler bypasses Worker: {path + suffix}"

    docs = {"/", "/index.html", "/connect.html", "/reference.html", "/.nojekyll", "/.well-known/acme-challenge/example"}
    for directory in ("assets", "archive"):
        docs.update("/" + str(path.relative_to(ROOT / "docs"))
                    for path in (ROOT / "docs" / directory).rglob("*") if path.is_file())
    for path in sorted(docs):
        for suffix in ("", "?v=1"):
            assert not worker_handles(path + suffix), f"Documentation intercepted by Worker: {path + suffix}"
    print(f"Validated {len(paths)} API/auth paths and {len(docs)} documentation paths, with query strings")


if __name__ == "__main__":
    check()
