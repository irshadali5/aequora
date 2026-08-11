# Local, token-efficient AI context

Aequora's contributor workflow uses three complementary local tools. None of them changes the sync
protocol or becomes a runtime dependency of the published library crates.

```text
natural-language intent -> Octocode hybrid RAG + GraphRAG -> bounded source line ranges
exact symbol or error   -> RTK-filtered ripgrep           -> compact lexical matches
crate dependency query -> Guppy Cargo graph               -> compact verified edges
```

## Install and configure

RTK and Octocode are developer-machine tools. Guppy is pinned in the workspace and built through
the non-publishable `aequora-dev` crate.

```bash
# RTK: follow the upstream installer, then add Codex instructions if this file is reused elsewhere.
rtk --version

# Octocode: install its prebuilt release or `cargo install octocode`.
octocode --version

# Keep embeddings and retrieval local, enable relationship-aware retrieval, and bound output.
octocode config --graphrag-enabled true --max-results 8 \
  --chunk-size 1800 --chunk-overlap 120 --similarity-threshold 0.25 \
  --code-embedding-model fastembed:Xenova/all-MiniLM-L6-v2 \
  --text-embedding-model fastembed:Xenova/all-MiniLM-L6-v2
scripts/rag index
```

The active Octocode configuration uses the compact local MiniLM FastEmbed model, does not require
an API key, and keeps LLM processing disabled while indexing. The `scripts/rag` wrapper fixes the
retrieval threshold at `0.25`, returns at most the configured eight results per search mode, and
uses partial text snippets. Disable `[search.reranker].enabled` in Octocode's generated
configuration unless its separate local reranker model has also been installed.

The database and embedding cache live under Octocode's user-local data directory, outside Git.
`.codex/config.toml` starts the same index as an MCP server for trusted Codex clients; restart Codex
after installing or changing the server. `scripts/rag index` intentionally uses Octocode's
`--no-git` mode so uncommitted working-tree edits are not skipped by commit-based indexing.

## Retrieval workflow

Use semantic retrieval before opening broad modules:

```bash
scripts/rag query "where are rejected operations reconciled atomically"
scripts/rag code "database-neutral authority transaction boundary"
scripts/rag docs "release verification gates"
```

Results default to token-efficient text with partial snippets. Follow up with `rtk read` or
`rtk rg` on the returned files. Use `octocode search --expand` only for a specific symbol whose full
definition is necessary.

After a material refactor, rebuild the index before using it for repository-wide conclusions:

```bash
scripts/rag index
scripts/rag stats
```

## Guppy dependency evidence

The local utility parses Cargo metadata through Guppy rather than inferring dependency direction
from directory names or manifest text:

```bash
cargo run -q -p aequora-dev -- summary
cargo run -q -p aequora-dev -- graph aequora-client
cargo run -q -p aequora-dev -- check
```

`check` rejects transitive coupling between the client/server layers and their opposite database or
transport adapters. It complements `scripts/check-database-neutrality.sh`, which still proves the
four supported feature profiles compile.
