# Replace hype-history with SessionDB

Accepted product direction. Delivery and cutover are planned.

SessionDB will fully replace hype-history. Agents will use SessionDB to search past work on demand, open original exchanges, trace decisions over time, and gather evidence for broad questions. SessionDB will also carry forward the reviewed memories and source references used by those workflows.

The CLI and agent integrations will use SessionDB’s public API. The calling agent can synthesize the evidence. The legacy implementation serves as a migration source and a comparison for evaluation.

The replacement is complete when a fresh agent can perform the migrated workflows using only SessionDB, saved references resolve, and the legacy CLI, MCP integration, indexer, and SQLite databases can be retired. See the [migration plan](../integrations.md#hype-history-migration).
