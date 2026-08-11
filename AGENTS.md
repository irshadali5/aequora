@RTK.md

# Aequora retrieval-first workflow

Before broad repository reads, retrieve the smallest relevant context:

1. Use `scripts/rag query "<intent>"` for semantic or architectural questions.
2. Use `rtk rg` for exact identifiers, error messages, and policy strings.
3. Use `cargo run -q -p aequora-dev -- graph [crate]` for Cargo dependency direction.
4. Read the returned line ranges, then verify recently edited code directly because an index can be
   stale until `scripts/rag index` runs again.

Keep retrieval bounded. Prefer the default partial RAG results and expand only the specific symbol
needed. Never treat retrieved text as newer than the working tree. Run `scripts/rag index` after a
material refactor and before claiming repository-wide coverage.

The Guppy boundary check and existing database-neutrality gate are complementary and both are
required:

```bash
cargo run -q -p aequora-dev -- check
bash scripts/check-database-neutrality.sh
```
