# First build scope

Status: accepted storage slice; identity operations/count superseded by [0013](0013-instance-api-key.md). Date: 2026-11-16.

## Decision

Implement the [first-build slice](../first-build.md) using the 23 OpenAPI operations marked `x-first-build`. Prove source-based import, authenticated session/message access, native export, and recovery with Codex first. Then add Claude/Pi, watch mode, keyword search, and optional embedding work through the planned interfaces.

## Consequences

The full v1 contract remains the target, while the first running server reports its actual capabilities. Unimplemented routes/options fail explicitly. Team administration, project labeling, additional exports, and semantic retrieval do not need to be implemented before the first storage-to-CLI path is useful. The required decisions for that path are settled; later tuning and publication choices remain in the delivery plan.
