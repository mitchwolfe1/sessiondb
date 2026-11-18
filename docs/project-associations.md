# Project associations

A project groups related sessions. It is optional metadata, independent of the native session store, the directory where a session started, and instance access.

Status: optional labels and read filters are supported by the model. Project administration, manual mutation, and matching rules remain planned; examples of future assignment below are not current CLI commands.

A session started in a home directory can be associated with a project elsewhere:

```text
source root:        /home/ada/.claude/projects
starting directory: /home/ada
project:            payments
repository:         /home/ada/work/payments
```

The starting directory remains as recorded. Assigning a project updates the SessionDB catalog, without moving or rewriting the native session file.

## Collection and grouping

| Concept | Purpose |
| --- | --- |
| Source | A configured harness session store on a device, such as `~/.codex/sessions`. |
| Starting directory | The cwd recorded by the harness. Useful for browsing and optional matching rules. |
| Project | A logical group of sessions, optionally linked to repository URLs. |
| Access | The same instance key gives every client full access; labels do not partition content. |

The sync client collects selected native stores even when no project matches. An unassigned session has `project_ids: []`; a session covering several projects can have several associations. A project need not correspond to one filesystem directory.

## Assignment

```sh
sessiondb sources add --detect
sessiondb sync --watch
sessiondb sessions list --unassigned
sessiondb sessions update SESSION_ID --project payments
sessiondb sessions update SESSION_ID --project payments --project api-client
sessiondb sessions update SESSION_ID --clear-projects
```

The proposed update API accepts a replacement `project_ids` array. Omission leaves associations unchanged; an empty array clears them. It does not change `recorded_cwd`, source bindings, or instance access. Ordinary sync updates native content and preserves catalog edits.

V1 supports explicit assignment and optional configured directory matching rules. Rules can supply initial associations for a newly collected session; unmatched sessions are still imported. Several matches add several associations instead of blocking collection. Rules do not overwrite later manual edits or reclassify an already imported session on every sync.

Recorded repository metadata and paths mentioned in tool activity can inform suggestions. A textual mention of a repository is not conclusive evidence that the session belongs to that project. Automatic content-based classification is later work and is not required to collect or search history.

## Search and access

Project filters narrow the shared collection. Removing the final label makes a session unassigned; it does not delete it. All key holders retain full access regardless of labels. Use separate instances when content must be isolated.

Keyword and configured semantic search include accessible unassigned sessions by default. The `unassigned` filter selects sessions with no project associations. A session associated with several matching projects appears once in the results.

Projects have no archive lifecycle. Renaming a label does not stop collection or delete associated sessions.

See [the data model](data-model.md), [automatic sync](sync.md), and [access control](access-control.md).
