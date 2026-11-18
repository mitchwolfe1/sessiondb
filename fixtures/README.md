# Native session fixtures

This directory contains synthetic native records and expected normalization fields for SessionDB. None was copied from a real user's conversation. Code-looking tool inputs are inert data and must never be executed.

The [manifest](manifest.json) records each harness/schema family, input files, virtual native paths, expected output, and intentional malformed/tail conditions. Files have descriptive names on disk; `native_paths` supplies the filename/path that the native adapter receives, including Claude subagent layout.

## Cases

| Case | What it specifies |
| --- | --- |
| `codex-home-tools` | Home-directory capture, JSON function input, free-form custom input, and duplicate UI event suppression |
| `claude-fragments-tools` | Contiguous provider fragments merge, repeated usage is retained once, and wire user tool output becomes tool content |
| `pi-home-tools` | Native blocks and tool results, with ISO versus epoch-millisecond timestamps |
| `pi-branches-context` | Branch ancestry through non-message entries, context edits, extension messages, and compaction |
| `codex-inherited-prefix` | An exact bounded parent prefix remains a related history rather than duplicate child messages |
| `claude-subagent` | A child repeats the parent's native session ID but remains a distinct agent session |
| `claude-compaction` | Historical text remains; generated summary/meta records have harness origin |
| `codex-incomplete-tail` | Whole-file rejection for complete malformed lines and incomplete direct input; raw bytes preserved as negative evidence |
| `pi-artifact` | Native inline PNG data becomes an instance-key-protected normalized artifact reference |
| `codex-precision-unknown` | Whole-file rejection for malformed JSON arguments and an unsupported native extension |

Fixtures model Codex 0.169.0, Claude Code 2.1.283, and Pi session format 3/client 1.5.0 shapes. The unknown-extension case intentionally tests strict rejection. The positive `codex-precision` case ([native input](native/codex/precision.jsonl)) verifies valid large-integer tool calls/results without narrowing negative coverage. These are bounded contract examples, not a claim that an adapter has already passed against all versions.

## Expected output

Rejected inputs use exactly `{"case":"...","error":"unsupported_native_record"}`; successful expectations contain sessions. Expected messages use symbolic `key` and `parent_key` values instead of requiring a particular database UUID allocator. `native_ids` identifies source entries or an explicit `line:N` locator when the native item has no ID. Several native anchors can contribute to one canonical message.

`content` follows the API's `FullContentBlock` union. Expected fields cover canonical role/origin, tool links, retained usage metadata, structural events, and session relations. Optional branches list symbolic ancestry for the corresponding view. Artifact IDs are fixture bindings declared in the manifest, not IDs derived from a user's filesystem.

For inherited history, the byte boundary describes the exact serialized parent input. Missing dependencies remain explicit. The fixture does not turn an inherited message into a new claim by another agent.

Each native file is independently uploaded through the synchronous multipart API. Malformed or unsupported supplied records reject the whole file with HTTP 400 before storage/catalog mutation. Only accepted valid originals are retained. CLI active-prefix capture defers unfinished tails before upload; direct incomplete input rejects. Related-file fixture sets do not imply atomic multi-file acceptance. See [sync](../docs/sync.md).

## Validation

[expectation.schema.json](expectation.schema.json) validates the expected wrapper and references [the OpenAPI content schemas](../docs/openapi.yaml). Validation loads that YAML document into the local JSON Schema registry before resolving the relative reference; it requires no network schema fetch.

Static checks cover JSON/schema validity, native anchors, expected parent graphs, declared malformed lines, incomplete-tail annotations, fork offsets, artifact bindings, and the embedded PNG's structure. [Adapter tests](../crates/adapters/tests/fixtures.rs) compare actual normalization with these expectations. Add version-specific fixtures when behavior changes.

Do not regenerate expected results silently from a parser under test. A behavior change should update its documented normalization rule and expected fixture in the same reviewed change.
