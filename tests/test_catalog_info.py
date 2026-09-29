import json
import unittest
from pathlib import Path

import scripts.check_catalog_info as checker

ROOT = Path(__file__).resolve().parents[1]


class CatalogInfoTests(unittest.TestCase):
    def test_repository_catalog_is_valid(self):
        self.assertEqual([], checker.validate(ROOT))

    def test_unknown_top_level_field_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        data["manualCount"] = 3
        self.assertTrue(any("schema" in error for error in checker.validate(ROOT, data)))

    def test_missing_selector_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        target = None
        def find_selector(value):
            nonlocal target
            if isinstance(value, dict):
                if value.get("format") == "html" and "selector" in value:
                    target = (value, "html")
                    return
                if value.get("format") == "markdown" and "heading_path" in value:
                    target = (value, "markdown")
                    return
                for child in value.values():
                    find_selector(child)
                    if target:
                        return
            elif isinstance(value, list):
                for child in value:
                    find_selector(child)
                    if target:
                        return
        find_selector(data)
        self.assertIsNotNone(target, "catalog must exercise a document selector")
        source, format_name = target
        source.pop("selector" if format_name == "html" else "heading_path")
        self.assertTrue(any("selector" in error or "heading" in error for error in checker.validate(ROOT, data)))

    def test_duplicate_markdown_heading_path_is_ambiguous(self):
        self.assertEqual(2, checker._heading_matches("# Parent\n\n## Same\n\n## Same\n", ["Same"]))

    def test_stale_selector_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        target = None
        def find_selector(value):
            nonlocal target
            if isinstance(value, dict):
                if value.get("format") == "html" and "selector" in value:
                    target = (value, "html")
                    return
                if value.get("format") == "markdown" and "heading_path" in value:
                    target = (value, "markdown")
                    return
                for child in value.values():
                    find_selector(child)
                    if target:
                        return
            elif isinstance(value, list):
                for child in value:
                    find_selector(child)
                    if target:
                        return
        find_selector(data)
        source, format_name = target
        if format_name == "html":
            source["selector"] = "#catalog-section-that-does-not-exist"
        else:
            source["heading_path"] = ["Catalog section that does not exist"]
        self.assertTrue(any("selector" in error or "heading" in error for error in checker.validate(ROOT, data)))

    def test_invented_capability_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        data["examples"][0]["capability_refs"] = ["mcp_tool:invented_tool"]
        self.assertTrue(any("capability" in error for error in checker.validate(ROOT, data)))

    def test_invented_mcp_server_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        data["mcpServers"]["invented-server"] = {}
        self.assertTrue(any("MCP server" in error for error in checker.validate(ROOT, data)))

    def test_invented_hook_target_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        data["hooks"] = [{"target": {"path": "plugins/missing/hooks.json", "pointer": "/hooks/Unknown/0/hooks/0"}}]
        self.assertTrue(any("hook target" in error for error in checker.validate(ROOT, data)))

    def test_path_escape_is_rejected(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        data["examples"][0]["sources"][0]["path"] = "../outside.md"
        self.assertTrue(any("path" in error for error in checker.validate(ROOT, data)))

    def test_documented_platform_requires_sources(self):
        data = json.loads((ROOT / "catalog-info.json").read_text())
        key = next(iter(data["platforms"]))
        data["platforms"][key]["sources"] = []
        self.assertTrue(any("source" in error for error in checker.validate(ROOT, data)))


if __name__ == "__main__":
    unittest.main()
