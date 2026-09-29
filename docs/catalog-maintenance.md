# Maintain catalog metadata

The marketplace reads root-level `catalog-info.json`. The canonical package is
`plugins/trakt-mcp`; compatibility copies do not add capabilities. The sidecar,
review receipt, and validator stay outside the release archives.

## Validate a snapshot

```sh
python3 -m pip install -r requirements-catalog.txt
python3 scripts/check_catalog.py
```

Validation uses local files only. It does not fetch a schema, contact external
services, scrape pages, or execute plugin code. Installing dependencies is a
separate setup step. Use `--root` to check another repository snapshot.

The validator checks the approved marketplace schema and adds repository rules:
identity, required example fields, native references, evidence, source paths,
unique selectors, and example coverage for every native capability. Paths must
stay inside the repository, including after symlink resolution. Diagnostics omit
source values and file contents. Nested source references in extension fields
receive the same validation. Every native hook also needs a unique catalog note
that targets its exact declaration.

HTML sources use `format: html`, `mode: section`, and a CSS `selector` that
matches exactly one element with text. Markdown sources use `format: markdown`
and `mode: section` with a `heading_path` array. A path can be a unique suffix of
the heading hierarchy, as in the marketplace parser. CommonMark parsing handles
fenced code and Setext headings. `mode: lead` selects text between the first and
second headings. Empty sections fail. Example evidence can also cite a whole
source file with a path-only reference.

`documented` and `unsupported` platform claims require source evidence.
`not_documented` remains a separate status. Illustrative examples require the
`reviewed_illustration` classification and matching `source_digests`; they do
not claim execution. This validation cannot determine whether prose is true.
Reviewers must compare the claims with the selected sources.

## Review changes

The review receipt, `catalog-sources.lock.json`, records SHA-256 fingerprints of
the sidecar, selected sources, canonical package, source code, public pages,
root README files, changelogs, native compatibility manifests, runtime dependency
manifests and lockfiles, catalog parser requirements, the validator, the embedded OpenAPI
contract, and the compiled API operation catalog.
Changelog discovery searches all project directories recursively, excluding
conventional dependency, cache, and build trees such as `node_modules`, `target`,
and `.venv`. It recognizes prefixed and suffixed names such as
`project-changelog.md` and `CHANGELOG-2026.md`, plus extensionless `CHANGELOG`,
`CHANGES`, and `HISTORY` files. Detection excludes known code, data, and binary file extensions;
`CHANGELOG.txt` and `HISTORY.rst` also require review. Additions, removals, and content
changes require review.
This conservative scope can require reviewing metadata even when its text stays
accurate. The receipt contains no local paths, environment values, or timestamps.

1. Review affected overview, examples, support claims, prerequisites, and source
   selectors against current files. Keep descriptions in skill frontmatter.
2. Update curated text only where the source evidence warrants it. Keep safety
   limits and uncertainty. Set `changelog` to explicit null when no changelog
   exists; omitting the field fails validation.
3. Resolve schema, selector, or native reference errors. Missing capabilities
   need an accurate example or removal of a stale reference. If native manifest
   routing or Rust tool declaration syntax changes, update the static adapter
   and its tests; do not execute the plugin to discover its inventory. The tool
   function must return a literal array of `tool(...)` declarations. Other
   helpers, direct objects, and alternate return construction require review.
4. Run `python3 scripts/check_catalog.py --refresh-reviewed-sources` only after
   review. It checks metadata before writing fingerprints and never edits prose.
5. Review the diff and run the validator and relevant repository checks. Include
   the receipt and metadata changes in a signed PR authored as `swackhamer`.

## Schema provenance and updates

`schemas/upstream-info.schema.json` is an exact copy of the marketplace's
[approved schema](https://github.com/swack-tools/ai-plugin-marketplace/blob/437c642bdac19d744dbb26b79bc2e673bebf0691/catalog/upstream-info.schema.json).
The approved metadata schema version is 1. The validator checks the vendored
file's SHA-256 digest before validation. No moving branch is consulted in CI.
The source receipt is a review artifact, not a plugin release version.

To refresh the schema, select an approved immutable marketplace commit and copy
its schema bytes. Review the schema and marketplace selector semantics together.
Update the provenance link, `SCHEMA_SHA256`, and the supported version check in
`scripts/check_catalog.py` when required. Match parser versions in
`requirements-catalog.txt` to the reviewed marketplace dependencies. Pin their
complete dependency closure, including JSON Schema reference handling. Resolve
updates with `uv pip compile requirements-catalog.txt --python-version 3.12`
and review the resulting version pins before changing the requirements. Run valid
and invalid regression cases, review the sidecar, and refresh its receipt in
the same PR. Do not relax checks just to accept an unreviewed schema change.

## Release procedure

PR checks validate the checked-out commit before packaging. The release job
checks out `github.sha`, verifies the tag and version, and validates metadata
before building or publishing assets. Both paths fail on invalid selectors,
schema violations, unknown capabilities, or changed source fingerprints.

Merge the signed PR after required checks and review pass. Then tag
the matching release commit through the authorized release procedure. Do not
retarget a published tag to repair metadata. Fix failures in a new reviewed PR
and follow the normal release policy. Tag CI never refreshes fingerprints,
rewrites metadata, or pushes commits. With `changelog: null`, GitHub's generated
release notes remain the fallback.
