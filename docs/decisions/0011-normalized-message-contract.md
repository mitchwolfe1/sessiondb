# Normalized message contract

Status: accepted. Date: 2026-11-16.

## Decision

Use the typed content union and normalization rules in [normalized content](../normalized-content.md), with separate full-content and preview representations. Inputs explicitly distinguish JSON from text; tool results retain success/error/unknown status and unique call links. Preserve unrecognized native content as opaque data.

Claude fragments with a common native provider message are coalesced along the same branch, while their repeated usage is retained once. Other provider item boundaries remain explicit. Keep inherited parent history as an authorized relation rather than duplicating it as new child messages.

## Consequences

API draft `0.3.0` replaces the earlier permissive content object. The [fixture pack](../../fixtures/README.md) defines expected normalized fields before adapter implementation. Preview expansion uses a revision and standard conditional HTTP reads. Available reasoning is distinguished from opaque signatures or hidden reasoning that was not captured.
