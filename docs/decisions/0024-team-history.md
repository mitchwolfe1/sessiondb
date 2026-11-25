# Team history in one collection

Status: accepted. This extends source provenance and preserves the shared-key model in [ADR 0013](0013-instance-api-key.md).

Each teammate chooses a stable handle and uses it on their computers. Each source can carry one collecting member. Sessions inherit the member and device of the source that first published that session. Byte-identical copies continue to share one session. A copy on another machine is insufficient evidence that the second person did the work. Member attribution describes the collection context. Message role, origin and the original exchange establish who said or did what.

The client stores the handle per server. On sync, an explicitly configured handle labels new stores and claims matching unlabelled stores. A conflicting label requires an explicit correction naming the previous value. Operators can also label individual existing sources by ID. Migration 0007 initially leaves all members unknown. Reading through the source association immediately applies a confirmed label to old and new history, including existing citations and embeddings.

Search defaults to the entire collection. A member or exact device filter narrows all retrieval modes and their coverage before ranking. Agents choose scope per lookup. Shared operational procedures can come from any colleague. A person's past decisions call for a member filter. A local machine or browser profile calls for the current member and device. Historical actions remain evidence to check against the current task.

All key holders have identical access to originals, indexed content and source labels. One instance serves the team under its existing key.
