# Trakt operation catalog

`catalog.json` is the dispatch allowlist generated from the [official API reference index](https://docs.trakt.tv/reference/llms.txt). Every operation links to its official endpoint page. The generator reads the OpenAPI definition embedded in that page and keeps the HTTP method, path, operation ID, categories, and request schemas. It does not copy response examples or lengthy reference prose.

The catalog describes the documented API surface. It does not claim that every operation works for every account. Account permissions, VIP features, ownership, private profiles, region restrictions, and upstream rate limits still apply. First-party-only operations remain discoverable with an unavailable status. OAuth credential operations are assigned to managed authentication rather than exposed as raw API calls.

## Refresh

Install and authenticate the Firecrawl command-line tool separately, then run from the repository root:

```sh
python3 scripts/update_trakt_catalog.py
python3 scripts/update_trakt_catalog.py --check
python3 scripts/update_trakt_catalog.py --offline-check
```

The first command reuses cached official pages under ignored `.firecrawl/trakt-catalog/`. Use `--refresh` to refetch the index and every endpoint. Requests run with at most two workers and are paced to stay below the observed request rate limit. Rate-limit errors receive bounded retries. A failed or incomplete refresh does not replace the committed catalog. Large captures can also populate the same ignored source cache with the official Firecrawl batch-scrape API, using `maxConcurrency: 2`, before running the generator.

`--offline-check` requires no network, cache, or credentials. It checks operation/source counts, unique identifiers, method/path pairs, source coverage and digest, resolved schemas, availability reasons, and matching path parameters against the committed compact `sources.json` inventory.

The generated output is deterministic for the same source pages. `source_index_sha256` hashes the sorted endpoint URL inventory, not the prose of the index. No API credentials are stored in this directory or passed to Trakt by the generator.

## Schema interpretation

- Request schemas resolve local OpenAPI references and translate OpenAPI `nullable` into JSON Schema `anyOf`.
- Descriptions and examples are removed from schemas. Properties whose names are `description`, `title`, or `examples` remain ordinary input properties.
- Objects with explicitly documented properties reject unknown fields unless upstream explicitly permits additional properties. This local safety restriction is recorded in operation metadata. Some upstream schemas use `oneOf` for identifiers, so supply a single identifier such as `ids.trakt`, not a copied object containing several IDs.
- Other missing upstream constraints remain missing; the catalog does not invent limits or imply complete upstream validation.
- A request body with schema-level required properties is required even when the upstream OpenAPI document omits the request-body required flag.
- OAuth Required and OAuth Optional markers in the endpoint reference determine authorization metadata. Some upstream security alternatives are broader than the endpoint's documented behavior.
- Calendar `my` targets require a connected account; calendar `all` targets use the global calendar. Personal calendars include watched, collected, and watchlisted items, so they are not equivalent to a watched-only filter.
- Paginated operations normalize `page` and `limit` to positive integer inputs, with local MCP bounds of 4,294,967,295 pages and 100 items per page. Missing pagination parameters are added only when the reference marks the endpoint as paginated. Each affected operation carries a normalization note.
- Pagination metadata records whether the endpoint documents pagination or declares a page parameter. It does not promise that an upstream array is paginated or complete.

Review changes to the inventory, request schemas, and availability before publishing a refreshed catalog. Run the repository's catalog and dispatch tests after refreshing.
