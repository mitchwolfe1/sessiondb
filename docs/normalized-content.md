# Normalized content

SessionDB normalizes native records into messages, typed content blocks, structural events, and session relationships. Original files remain available independently of that projection.

Status: implemented baseline for API draft `0.3.0`. The [OpenAPI schemas](openapi.yaml) and [synthetic fixtures](../fixtures/README.md) define the shapes and expected adapter output.

## Messages and content blocks

A message has a canonical role, recorded native role, origin, sequence, conversation parent, normalized revision, and ordered content. A model response can contain several blocks; a tool call can also be a separate assistant message when the native format records it that way.

| Block type | Required payload | Meaning |
| --- | --- | --- |
| `text` | `text` | Recorded text. |
| `reasoning` | `text`, optional `redacted` | Available reasoning text; never a reconstruction of hidden reasoning. |
| `tool_call` | `call_id`, `name`, `input` | A tool invocation. A missing native link ID is null. |
| `tool_result` | `call_id`, `status`, `content` | Output blocks linked to an invocation when unambiguous. Status is success, error, or unknown. |
| `json` | `value` | Explicit native structured JSON; all JSON values, including null, are valid. |
| `artifact` | `artifact_id`, `media_type`, `kind` | A file/image/audio/video reference served through instance-key-protected artifact endpoints. |
| `unknown` | `native_type`, `data` | An unrecognized native block retained as opaque data. |
| `content_reference` | `content_url`, `revision`, `block_index`, `original_type` | A bounded preview's reference to a full block; absent from full-content responses. |

Tool result output can contain text, JSON, artifact, or unknown blocks. Calls/results are paired by a unique native call ID within the appropriate session/source history. If a result cannot be paired unambiguously, its canonical `call_id` is null and a warning retains the diagnostic; timing alone is not a link.

## Tool inputs

Tool input is an explicit union:

```json
{"encoding":"json","value":{"command":"pwd"}}
```

```json
{"encoding":"text","value":"text(2 + 2);"}
```

Native function argument JSON strings are decoded as JSON when valid. A malformed argument string is retained as text with a warning. Free-form custom tool input remains text, even if it resembles a JSON document. Tool output strings are not automatically parsed merely because they look like JSON.

Use arbitrary-precision JSON handling for native values; do not round a large integer through a floating-point representation. The selected serde_json features are recorded in [the dependency baseline](validation/reference-stack.md).

## Provider normalization

**Codex:** preserve model-facing messages and tool items, with tool-only items represented as assistant messages. A corresponding UI event does not create a second visible message. Lifecycle, compaction, and rollback records remain events. Preserve free-form custom-tool input separately from function-argument JSON.

**Claude Code:** merge contiguous fragments of the same native provider message along the same branch, retaining their native anchors and block order. Repeated usage snapshots from those fragments are retained once rather than summed. A wire user message containing tool results becomes canonical tool output; mixed text/results are split in native block order. Missing or conflicting provider identities remain separate records with a diagnostic.

**Pi:** retain the native message's ordered text/reasoning/tool blocks. Convert `toolResult` to the canonical tool role. Bridge non-message tree entries when resolving conversation parents. Context edits, model changes, custom state, and compaction remain events; extension-injected messages have harness origin.

Origins are `native_user`, `model`, `tool`, `harness`, or `unknown`. `native_user` describes a native record, not proof that a human authored it. Available provider usage remains provider-shaped metadata on the appropriate canonical response; the API does not silently turn incompatible provider counters into universal totals.

## History and inheritance

The default message list is historical data, including alternate branches. `leaf_id` selects conversation ancestry. Context edits do not delete old historical text, and compaction summaries/meta inputs are labeled harness-origin.

An inherited parent rollout is a bounded relation, not duplicated into the child's own message list as fresh messages. `history_boundary` records the rollout ID and exclusive ordinal/byte boundary; the native legacy field named `thread_id` can actually identify a rollout. Clients can retrieve the parent separately with the same instance key, and native bundles include available dependencies with a manifest. Unavailable dependencies are reported. Native Claude subagent files remain distinct even when they repeat the parent's session ID.

Optional message `source_refs` records identify file generations and exact native line/byte ranges. These references describe where content was recorded; original files use the same instance-key boundary as all data.

## Previews and expansion

Message previews are bounded to 64 KiB of UTF-8 content. Text can be shortened at a valid UTF-8 boundary. Oversized structured blocks are replaced by `content_reference`; tool input JSON is never cut into an invalid or misleading partial value. `truncated` is true whenever a preview omits content.

`GET /messages/{id}/content` returns full canonical blocks with their revision. A client expanding a preview sends the quoted revision in `If-Match`; a changed revision returns `412`, after which the client reloads the message. Responses expose the same strong revision in `ETag`. Artifact bytes remain separate from full normalized JSON.

Unknown native content is data. Render it as escaped content or a reference, and retain its original type rather than guessing a new operation or instruction.

## Fixtures

The [fixture manifest](../fixtures/manifest.json) supplies native paths and expected normalized fields without prescribing database-generated UUIDs. Its cases cover home-directory launches, JSON/text inputs, repeated fragments, subagents, inherited history, branches/context edits, compaction, artifacts, and malformed/incomplete capture. Schema checks validate the expectations; [adapter tests](../crates/adapters/tests/fixtures.rs) compare actual normalized output with them.
