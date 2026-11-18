# Reference storage and dependency baseline

Status: dependency/provider baseline accepted; signed staging and asynchronous finalization below are historical, superseded by [0014](0014-sessions-and-synchronous-files.md). Date: 2026-11-16.

## Decision

Use SeaweedFS 4.54 for the self-hosted S3 reference deployment and `aws-sdk-s3` 1.159.0 for the storage client. Use path-style requests, explicit region/endpoint configuration, and the pinned SDK behavior `v2026_11_10`. Require explicit SHA-256 on signed uploads and verify accepted file bytes independently of an ETag. Preserve native blobs uncompressed in v1 so byte ranges address original bytes.

Use the compiler/dependency baseline tested in [reference validation](../validation/reference-stack.md). Rust 1.94.1 is the initial tested minimum; commit the real workspace lockfile when application implementation begins.

## Rationale

The selected pair passed actual signed PUT/GET, metadata length, byte-range, conditional creation, checksum rejection, conditional copy, restart persistence, and delete checks. Copying staging data to a server-only key kept accepted bytes unchanged when the staging object changed. The official client exposes the S3-specific controls needed by imports.

## Consequences

Historical consequence: finalization used an asynchronous promotion job and `accepted_at`, with leases and renewable targets. ADR 0014 replaces that protocol with synchronous API upload; these checks do not validate the replacement. Application generations remain independent of bucket versioning. No live cloud account or model endpoint was used in the compatibility checks.
