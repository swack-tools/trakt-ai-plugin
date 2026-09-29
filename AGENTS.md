# Repository guidance

## Package and behavior boundaries

- Use `plugins/trakt-mcp` as the canonical plugin package. Root manifests and
  marketplace declarations provide compatibility and discovery; do not count
  them as extra skills, commands, hooks, or MCP servers.
- Keep the Claude, Codex, and portable manifests consistent. Preserve their
  client-specific archive layouts and the standalone skills' MCP dependencies.
- Preserve explicit write intent, write scope, confirmation, and ownership
  checks. Existing read-only tokens stay read-only. Never retry an uncertain
  write automatically. Report incomplete pagination and uncertain availability.
- Skills provide instructions; an authenticated MCP connection provides tools.
  Example prompts illustrate usage. They do not establish execution or public
  directory approval.
- Never track or print credentials, local user paths, private logs, session
  records, or environment values. Validation errors must not echo source data.
- Keep changes focused, preserve unrelated work, and update relevant docs when
  behavior, public interfaces, installation, or runtime requirements change.

## Catalog metadata maintenance

Keep `catalog-info.json` at the repository root, outside the packaged plugin.
Native files remain authoritative for inventory and counts. Keep skill
descriptions in `SKILL.md` frontmatter. Do not add manually maintained counts,
local paths, guessed commits, or generated claims to the sidecar.

Review the sidecar when any of these change:

- Skill names, frontmatter descriptions, behavior, additions, or removals.
- Commands, hook handlers, MCP servers, tools, or their behavior and interfaces.
- Example prompts, prerequisites, expected behavior, or client invocation.
- Referenced README files, documentation, headings, HTML selectors, or paths.
- Platform support, installation guidance, or runtime requirements.
- The presence or location of a changelog.

Curated prose requires human review. CI must not invent or rewrite descriptions,
examples, support claims, or changelog prose. Use `not_documented` when support
lacks evidence. Keep `changelog` null when no changelog exists; releases then
provide generated release notes.

Install validation dependencies and run the offline schema, native inventory,
selector, and source freshness checks:

```sh
python3 -m pip install -r requirements-ci.txt
python3 scripts/check_catalog.py
```

After reviewing affected claims and fixing missing or ambiguous selectors, run
`python3 scripts/check_catalog.py --refresh-reviewed-sources`. Review and commit
the resulting `catalog-sources.lock.json` with the source changes and any
metadata edits. This command updates fingerprints only. Never run it in CI or
use it to bypass a prose review. See [catalog maintenance](docs/catalog-maintenance.md)
for selectors, schema updates, review scope, and release failure recovery.

## Checks and delivery

Run the relevant checks before committing. For catalog changes, run:

```sh
python3 scripts/check_catalog.py
python3 -m unittest discover -s tests -p '*.py' -v
python3 -m ruff check scripts tests docs
python3 scripts/check_plugin.py
python3 scripts/update_trakt_catalog.py --offline-check
python3 scripts/check_routes.py
actionlint
```

For documentation changes, build and check a fresh output directory with
`docs/build.py` and `docs/check.py`, then run `vale sync` and
`python3 scripts/check_prose.py`. For Rust or MCP changes, also run the Rust,
WASM, and local MCP integration checks in `.github/workflows/checks.yml`.
For packaging changes, run the portable schema, official client, and repeatable
archive checks in `.github/workflows/plugin.yml`.

Report exactly which checks passed, failed, or remain unverified. Local results
do not establish GitHub CI success. Preserve the protected branch and CI rules
in `CI_SECURITY.md`.

Use signed commits authored as `swackhamer`. Verify the local signature and
GitHub's verification status and author for every PR commit. Do not assume
signing succeeded. Open a focused PR; do not merge or tag without authorization.

PR checks and tag releases must validate the checked-out metadata before
building or publishing plugin assets. A failed check blocks publication. Correct
failures in a reviewed, signed PR, run validation, and merge before tagging the
matching release commit. Tag workflows must not rewrite metadata or push commits.
