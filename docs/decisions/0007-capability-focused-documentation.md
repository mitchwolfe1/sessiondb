# Capability focused documentation

Status: accepted; identity-feature assumptions superseded by [0013](0013-instance-api-key.md). Date: 2026-11-15.

## Decision

Describe SessionDB as a database for coding-agent sessions. Lead with what it does: store native sessions, sync updates automatically, provide consistent queries and exports, and support search through a CLI, HTTP API, and SDK.

Organize introductory documentation around these capabilities and tasks. Shared deployments, users, teams, and permissions belong in the relevant configuration and reference pages. Context about potential customers does not define an exclusive product audience. Rust remains the implementation language; language-specific details belong in architecture, contributor, and SDK references.

## Rationale

Database and developer-tool documentation helps readers understand data, interfaces, and workflows before introducing administration. PostgreSQL and MLflow are useful references for this structure. SessionDB should be understandable as a useful tool whether it is accessed by one person, a script, an application, or several people.

## Consequences

This clarifies the framing of [0001](0001-product-and-rust.md) and the rationale of [0006](0006-team-access.md). The API and access model remain specified in their technical references. Introductory pages emphasize capabilities, and contributor guidance keeps that focus as documentation grows.
