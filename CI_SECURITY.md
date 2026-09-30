# CI and dependency security

Pull requests run Rust, Python, MCP, plugin, documentation, CodeQL, and dependency checks. Production deployment runs only after a push to main. Documentation deployment also requires a documentation or Pages workflow change.

## Protected main branch

The repository requires an up-to-date branch and these GitHub Actions checks:

- `rust-and-mcp`
- `python-and-workflows`
- `validate`
- `documentation`
- `analyze (javascript-typescript)`
- `analyze (python)`
- `dependency-audit`

Main also requires one approving review from someone other than the last person to push changes. New commits dismiss stale approvals. Review conversations must be resolved. Administrators follow these rules, and force pushes and branch deletion are turned off. An automated Codex comment reporting no findings does not replace an approving GitHub review.

The expected settings are in `.github/repository-policy.json`. An operator with repository administration read access can verify the live settings:

```sh
python3 scripts/check_repository_policy.py --output /tmp/trakt-github-policy.json
```

The command only reads GitHub settings. It fails when branch protection, required checks, token permissions, runner configuration, or dependency security settings differ. Its output contains configuration evidence, not credentials. Pull request tests use deterministic fixtures to exercise this verifier; they do not claim to verify live settings or require administrator credentials.

## Dependency evidence

The dependency check runs on every pull request and fails on known advisories or scanner errors:

- OSV Scanner checks every package in `Cargo.lock` and `package-lock.json`.
- `npm audit` checks the npm registry's advisories.
- `pip-audit` resolves `requirements-ci.txt` and checks its direct and transitive Python packages against PyPI advisories.

The job uploads JSON results in the `dependency-advisory-reports` artifact for 30 days, including results from failed scans when available. It does not suppress advisories. A passing scan means no known advisories were returned at that time; it does not prove that dependencies have no vulnerabilities.

Rust builds and tests use the committed lockfile with `--locked`. GitHub Actions use commit pins. The OSV binary download has a pinned version and checksum. Checkout credentials are not persisted. Jobs have time limits, PR jobs use read-only source permissions, and CodeQL has the additional permission needed to upload findings.

Dependabot checks GitHub Actions, Cargo, npm, and Python weekly and opens update pull requests. These update checks are dependency maintenance, not scheduled test workflows. Updates must pass the same PR checks and review requirements. Dependabot alerts and security updates are enabled in repository settings. No automatic merge bypass is configured.

## Release notifications

Version tag pushes publish plugin archives with the repository's scoped GitHub token. After publication succeeds, a separate job requests the marketplace build. That job has no GitHub token permissions and checks out no source. Its single dispatch step uses `MARKETPLACE_DISPATCH_TOKEN` with Actions write access to `swack-tools/ai-plugin-marketplace`.

The workflow policy permits this secret only in that step. It requires the `release` dependency and fixes the dispatch destination to `pages.yml` on the marketplace's `main` branch. Policy tests reject extra steps, other credentials, broader permissions, and different destinations. PR jobs remain credential-free. See [marketplace refresh setup](.github/MARKETPLACE_REFRESH.md).

## Scope and remaining limits

The owner selected public advisory scanners instead of Endor Labs. Endor authentication, namespace access, malware intelligence, and proprietary dependency risk evidence remain unverified. The public scan results do not substitute for those signals.

The repository's policy tests and live settings verifier provide explicit checks for this project. They are not an Endor policy evaluator. Current settings and workflow evidence do not establish how every historical workflow run behaved.

Sources include [GitHub branch protection](https://docs.github.com/en/rest/branches/branch-protection), [Dependabot configuration](https://docs.github.com/en/code-security/reference/supply-chain-security/dependabot-options-reference), [OSV Scanner](https://github.com/google/osv-scanner), and [pip-audit](https://github.com/pypa/pip-audit).
