# Repository guidance

## Canonical package and catalog metadata

The canonical distributable plugin package is `plugins/trakt-mcp`. Don't count root compatibility files as extra native capabilities. Keep curated marketplace metadata in root `catalog-info.json`, outside release archives. Native skills, Model Context Protocol server declarations, and statically registered tools define the inventory.

Update `catalog-info.json` when a maintainer adds, removes, or renames a skill, or changes its frontmatter description or behavior. Review it when a command or hook action changes, or when a Model Context Protocol server or tool changes. Update examples, prerequisites, expected results, platform invocations, and source selectors when their supporting documentation changes. Review the sidecar when platform support, installation guidance, runtime requirements, or changelog status changes. Use repo-relative evidence paths. Keep prose human-reviewed. CI validates metadata but never writes or rewrites it. Mark platforms `not_documented` without evidence and leave `changelog` null when none exists so GitHub Release notes remain the fallback.

Run `python3 -m pip install -r requirements-catalog.txt` and `python3 scripts/check_catalog_info.py`. The checker validates the approved, vendored marketplace schema, its pinned digest, source paths and selectors, and references against the canonical package. Refresh `.github/schemas/upstream-info.schema.json` only from an approved marketplace revision. Update the exact commit and digest in `scripts/check_catalog_info.py`. Update this guidance and validate in a reviewed PR. CI mustn't follow a moving branch.

When a source change affects the catalog, update it in a reviewed, signed PR. Run the checker and repository checks, then merge the PR and tag the matching release commit. PR and tag workflows validate the exact commit before release packaging and publication. A validation failure blocks the release. CI must not rewrite the sidecar or push commits. Keep `changelog` null when no repository changelog exists. GitHub Release notes supply the notes.

## Change quality

- Preserve user-authorized boundaries for Trakt access and writes. Never make changes to a user’s library unless the user explicitly asks for that operation.
- Keep per-user OAuth 2.0, tenant isolation, and least-privilege scopes intact. Never commit credentials, environment values, local paths, private logs, or session data.
- Keep Claude and Codex manifests and release archives aligned with `plugins/trakt-mcp`. Don't duplicate capability counts.
- Update docs and catalog metadata when behavior, interfaces, permissions, prerequisites, or platform support changes.
- Run relevant Rust formatting/tests/lints, Python tests/lints, package and workflow validation, and the catalog checker. Keep changes focused, preserve unrelated work, and report what ran and what remains unverified.
- Use commits authored as `swackhamer` and verify every commit signature before requesting review.
