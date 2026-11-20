# Lossless native JSON payloads

Status: accepted implementation correction. Date: 2026-11-18.

## Failure and decision

A concurrently pending Pi tool result contains a NUL character in its text. It is valid JSON as `\u0000`, but PostgreSQL JSONB rejects that escape. A minimal synthetic native upload reproduced HTTP 500 with PostgreSQL's `unsupported Unicode escape sequence` error. Looking only at the first pending file misses failures in the client's concurrent upload window.

Use PostgreSQL `json` for opaque message content, provider usage, and native event data. Forward migration `0003_native_json_payloads.sql` converts existing values without changing their data. The initial migration remains immutable; typed relation/source-reference metadata remains JSONB.

Bind these payloads with the internal `NativeJson` codec, which declares PostgreSQL JSON and serializes directly into the SQL argument buffer. SQLx's ordinary `Json` parameter defaults to JSONB, so changing column types alone is insufficient. Existing JSON decoding can read either type. Do not strip, replace, or reject otherwise supported native content to accommodate a database encoding limitation.

## Consequences

Derived display titles replace control characters with spaces because they are human-readable relational metadata. Original message text remains intact in JSON and S3; this is not transcript sanitization.

Read opaque payloads as JSON and process them in the application. Casting them to JSONB or extracting NUL-bearing strings as PostgreSQL text reintroduces the limitation. Relational metadata filters remain unchanged; payload-specific indexes or text projections are separate future designs.

Verify tool-text preview/full-content, provider usage, event data, exact original bytes, and atomic rejection of an unsupported overwrite using synthetic control-character records. Test both a fresh catalog and forward upgrade preserving existing conversations. No credentials, warnings, revisions, or new user-facing settings are added.
