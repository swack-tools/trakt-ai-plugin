#!/usr/bin/env python3
"""Build client-specific Claude and Codex ZIPs for a GitHub Release."""
import hashlib
from pathlib import Path
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLUGIN = ROOT / "plugins/trakt-mcp"
OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dist/release"
EXCLUDED_PARTS = {".git", "__pycache__", "node_modules"}
EXCLUDED_NAMES = {".DS_Store", "Thumbs.db"}


def archive(target: Path, client: str) -> None:
    files = []
    for source in PLUGIN.rglob("*"):
        if not source.is_file() or source.is_symlink():
            continue
        rel = source.relative_to(PLUGIN)
        if any(part in EXCLUDED_PARTS for part in rel.parts) or source.name in EXCLUDED_NAMES:
            continue
        if client == "claude" and (rel.parts[0] == ".codex-plugin" or rel.as_posix() == "plugin.json" or rel.as_posix() == "mcp.json"):
            continue
        if client == "codex" and (rel.parts[0] in {".claude-plugin", ".codex-plugin"} or rel.as_posix() == ".mcp.json"):
            continue
        files.append((source, rel.as_posix()))
    with zipfile.ZipFile(target, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for source, rel in sorted(files, key=lambda item: item[1]):
            info = zipfile.ZipInfo(f"trakt-mcp/{rel}", date_time=(2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            zf.writestr(info, source.read_bytes())


def main() -> None:
    from check_plugin import validate
    validate(PLUGIN)
    OUT.mkdir(parents=True, exist_ok=True)
    assets = []
    for client in ("claude", "codex"):
        target = OUT / f"trakt-mcp-{client}.zip"
        archive(target, client)
        assets.append(target)
    sums = "".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in assets)
    (OUT / "SHA256SUMS").write_text(sums)
    print(f"Created {len(assets)} release ZIPs and SHA256SUMS in {OUT}")


if __name__ == "__main__":
    main()
