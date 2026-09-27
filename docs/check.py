#!/usr/bin/env python3
"""Validate generated local links, page landmarks, and the immutable README."""
import argparse
import hashlib
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

SNAPSHOT_SHA256 = "26e0af0f31c19d1cca042be070799dbb412fcd4a84b745d31034e826f292faa2"


class Page(HTMLParser):
    def __init__(self, source):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.links = []
        self.h1_count = 0
        self.landmarks = set()
        self.errors = []
        self.feed(source)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            if attrs["id"] in self.ids:
                self.errors.append(f"Duplicate ID: {attrs['id']}")
            self.ids.add(attrs["id"])
        if tag == "h1":
            self.h1_count += 1
        if tag in {"main", "header", "footer", "nav", "title"}:
            self.landmarks.add(tag)
        for key in ("href", "src"):
            if key in attrs:
                self.links.append(attrs[key])


def check(root):
    root = root.resolve()
    pages = {p: Page(p.read_text()) for p in root.rglob("*.html")}
    assert len(pages) == 3, "Expected overview, connect, and reference pages"
    checked = 0
    for path, page in pages.items():
        assert not page.errors, f"{path}: {page.errors}"
        assert page.h1_count == 1, f"{path}: expected one h1"
        assert page.landmarks == {"main", "header", "footer", "nav", "title"}, f"{path}: missing landmarks"
        for link in page.links:
            parts = urlsplit(link)
            if parts.scheme or parts.netloc:
                assert parts.scheme == "https", f"Unexpected external link: {link}"
                continue
            assert not parts.path.startswith("/"), f"Use project-relative paths: {link}"
            target = (path.parent / unquote(parts.path)).resolve() if parts.path else path
            assert target.is_relative_to(root), f"Link escapes site: {link}"
            assert target.is_file(), f"{path.name}: missing target {link}"
            if parts.fragment:
                assert target in pages and unquote(parts.fragment) in pages[target].ids, f"{path.name}: missing anchor {link}"
            checked += 1
    snapshot = root / "archive/README-37ac12d.md"
    assert hashlib.sha256(snapshot.read_bytes()).hexdigest() == SNAPSHOT_SHA256, "Historical README changed"
    for path in root.rglob("*"):
        assert not path.is_symlink(), f"Pages artifact must not contain symlinks: {path}"
    print(f"Validated {len(pages)} pages, {checked} local links, landmarks, and exact README snapshot")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("site", type=Path)
    check(parser.parse_args().site)
