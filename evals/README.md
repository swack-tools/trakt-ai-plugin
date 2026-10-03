# Plugin evals

These cases run with `claude plugin eval`. Each case sends one prompt to Claude Code with the
plugin loaded and grades the reply and tool calls. The cases cover all seven skills and a
negative trigger.

All Trakt tools are mocked under `mocks/trakt/`, so runs never contact the hosted service or a
Trakt account. Write tools return `write_authorization_required` by default; the write cases
override them with success responses in their own `mocks/` directories. The `expect:` guards stop a run
that sends the wrong target or omits `confirmed: true`.

Run the suite from the repository root:

```sh
claude plugin eval .
```

Each case runs three times with the plugin and three times without it, and judge graders add
model calls. To check one case cheaply, run:

```sh
claude plugin eval . --case <case-name> --runs 1 --ablation none
```

Evals need a Claude Code login. Results go to `evals/results/`, which Git ignores.

`mocks/trakt/_tools.json` mirrors `protocol::tools()` in `src/mcp/protocol.rs`, without output
schemas because mocks return text only. The `manage-library-rating` fixtures mirror
`api/trakt/catalog.json`. Regenerate both when tool definitions or catalog operations change.

The runner refuses hard-linked files anywhere in the plugin directory. Delete a local Cargo
`target/` directory before running evals.
