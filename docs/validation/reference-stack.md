# Reference stack validation

Historical record: the original first-build storage and OIDC authentication choices were checked on 2026-11-16 using disposable local services and synthetic data. These initial experiments ran outside the application checkout during the documentation phase. The repository now also contains the [initial implementation](../implementation.md) and its integration suite.

These results establish historical dependency/protocol compatibility, not validation of the new instance-key model. [ADR 0013](../decisions/0013-instance-api-key.md) supersedes all OIDC assumptions below. Keycloak, browser probes, and identity libraries are no longer runtime prerequisites. No instance-key cutover tests were run for this documentation update; integrated results must be recorded separately after execution. These experiments did not exercise application adapters or production performance.

## Historically tested components

| Component | Selected/tested version |
| --- | --- |
| Compiler baseline | Rust 1.94.1, Rust 2024 edition, Linux x86-64 |
| S3 reference | SeaweedFS 4.54 |
| S3 client | aws-sdk-s3 1.159.0, behavior version `v2026_11_10` |
| Identity reference | Keycloak 26.8.2 |
| Browser probe | Chromium through agent-browser 0.22.2, isolated session |
| HTTP/runtime | reqwest 0.13.5 and Tokio 1.54.1 |
| Service/database/CLI | axum 0.8.9, SQLx 0.9.0, clap 4.6.7 |
| Serialization | serde 1.0.229, serde_json 1.0.154 with `arbitrary_precision` and `raw_value` |
| Identity libraries | openidconnect 4.1.0, jsonwebtoken 12.0.0 with the selected crypto backend |

The temporary compatibility crate compiled the selected direct dependency set and ran the S3 probe on Rust 1.94.1. This is the initial tested minimum, not an inference from the workstation's default compiler. The current application has its own committed Cargo.lock/toolchain. The superseded OIDC library brought reqwest 0.12.28 alongside the main client's 0.13.5; this was a historical compatibility observation, not a current dependency requirement.

## Historical reference images

```text
chrislusf/seaweedfs:4.54@sha256:4b42c212543a0acd4c0e675215ecdcaf62a186ef98def76c7e12701871222d15
quay.io/keycloak/keycloak:26.8.2@sha256:c68e286cacd87b942ac1939a0a1481461109d6acd3a57dc496969a319a76bbf0
```

The storage service used a temporary persisted data directory and only a loopback-published S3 port. The identity service used a temporary imported realm and synthetic account. Existing host services and their databases were not used.

## Storage checks

| Check | Result |
| --- | --- |
| Official client with custom endpoint, region, and path-style addressing | Passed |
| Presigned PUT with explicit SHA-256 and returned request headers | Passed |
| HEAD reports the exact stored byte length | Passed |
| Presigned GET round-trips original bytes | Passed |
| Byte-range GET returns the requested source prefix | Passed |
| Conditional create of an existing key | Rejected with `PreconditionFailed`; original bytes remained intact |
| Incorrect SHA-256 payload | Rejected with `BadDigest` |
| Conditional server-side copy | Passed; stale source ETag rejected with `PreconditionFailed` |
| Staging changes after promotion | The accepted copy remained unchanged |
| Restart with the same data directory | Original object remained readable |
| Delete followed by absent-object HEAD | Passed |
| JSON integer beyond u64 survives selected serialization | Passed exact digit round-trip |

Use `RequestChecksumCalculation::WhenRequired` and `ResponseChecksumValidation::WhenRequired` explicitly, while supplying SHA-256 for uploads and verifying accepted bytes. This avoids accidentally depending on another provider's optional default checksum algorithm. Send every header returned with a presigned request; a URL alone is not always the full request.

SeaweedFS was selected as an existing single-node reference that passed the required operations. The official SDK gives the needed S3 signing controls. Other S3-compatible providers use the same checks before being listed as tested. Bucket versioning is not required for SessionDB's application file generations.

## Superseded OIDC authentication checks

| Check | Result |
| --- | --- |
| Discovery matches the configured issuer | Passed |
| Real browser login and public-client code exchange with S256 | Passed |
| Dynamic loopback port with registered `http://127.0.0.1/callback` | Passed |
| Callback state and ID-token nonce | Passed |
| Missing PKCE / incorrect verifier | Rejected |
| API access token signature and required identity/audience claims | Passed |
| ID token presented as API token | Rejected by audience |
| Wrong API audience/issuer and modified signature | Rejected |
| Expiry check with a real signed token and advanced reference clock | Rejected beyond the 30-second leeway |
| Refresh token exchange | Passed |
| Verified email needed for invitation matching | Present in reference configuration |

The tests found two configuration/transport details worth retaining: OAuth errors return through the callback, and Keycloak's `basic` client scope supplies the subject claim. Login used a real browser, while a Python JWT reference validator checked signatures/claims and an advanced clock exercised expiry. The selected Rust identity dependencies compiled in that experiment; the implementation later exercised them as recorded in [historical first-build validation](first-build.md). They have since been removed by the instance-key cutover.

The local issuer used development HTTP on localhost. Public deployments use HTTPS; this validation did not exercise a production TLS termination configuration. See [authentication](../authentication.md) for the replacement instance-key contract, not this historical OIDC setup.

## Sources

- [SeaweedFS reference release](https://github.com/seaweedfs/seaweedfs/releases/tag/4.54) and [S3 API](https://github.com/seaweedfs/seaweedfs/wiki/Amazon-S3-API).
- [AWS Rust presigning](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/presigned-urls.html) and [behavior versions](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/behavior-versions.html).
- [Keycloak container/import setup](https://www.keycloak.org/server/containers) and [OIDC endpoints](https://www.keycloak.org/securing-apps/oidc-layers).
- [Keycloak loopback redirect implementation](https://github.com/keycloak/keycloak/blob/26.8.2/services/src/main/java/org/keycloak/protocol/oidc/utils/RedirectUtils.java).
- [openidconnect](https://docs.rs/openidconnect/4.1.0/openidconnect/) and [jsonwebtoken](https://docs.rs/jsonwebtoken/12.0.0/jsonwebtoken/).

Native fixture shapes and expected normalization are documented separately in [the fixture pack](../../fixtures/README.md). Historical adapter conformance is recorded in [first-build validation](first-build.md); this reference-stack experiment itself did not test an adapter.
