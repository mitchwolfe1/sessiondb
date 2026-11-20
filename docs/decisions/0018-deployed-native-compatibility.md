# Native compatibility and deployed upgrades

Status: accepted. Date: 2026-11-18. Supersedes the undeployed migration assumptions in [0014](0014-sessions-and-synchronous-files.md) and [0015](0015-strict-native-records-and-simple-reads.md), not their schema or strict-acceptance contracts.

## Decision

Athena is deployed and holds accepted user history. Keep `0001_catalog.sql` immutable. All future catalog/schema repairs use new forward migrations; preserve existing IDs, originals, metadata, and client checkpoints. Never reset the database, volumes, migration ledger, or local sync state as an upgrade mechanism.

Support observed native Codex web-search actions, tool definitions, world-state records, and token-usage records. Support Claude mode/permission metadata, attachments, and AI titles. Known structural envelopes retain their complete native payload and source anchors; web searches produce recorded tool calls without fabricated results or call IDs. Unrecognized native kinds and malformed known records still reject the whole file.

Complete Codex AGENTS.md and environment-context setup envelopes remain in history with their native role/content but have harness origin. Default titles prefer recorded native custom/AI titles, then meaningful human text; blank lines are not titles. Do not hide genuine user questions that only mention instruction markers or mix human text with setup blocks.

Forward migration `0002_codex_instruction_origins.sql` repairs existing Codex origins and replaces a stored title only when it equals the previous derived default. Explicitly different manual titles remain unchanged. Original bytes and message text/IDs are not rewritten.

## Verification boundary

Use only sanitized structural evidence and synthetic fixtures in the repository. Validate both a fresh catalog and upgrade from migration 0001 with existing automatic/manual titles and preserved content. Real production errors still require actual diagnostics; do not disguise unsupported data or internal errors with warning fields, skipped records, or opaque unknown fallbacks.
