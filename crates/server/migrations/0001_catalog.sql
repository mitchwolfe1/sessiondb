-- SessionDB stores conversations and their original native files.
-- Sessions contain messages, structural events, artifacts, and optional projects.
-- Sources identify client session stores; file generations preserve native rewrites.
-- Objects describe immutable S3 bytes shared by originals and artifacts.
-- Sync accepts one file per request; there are no import batches or job queues.

-- Conversations: ordinary metadata is relational; variable native content is JSON.
CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    source_id uuid NOT NULL,
    native_key text NOT NULL,
    harness text NOT NULL CHECK (harness IN ('codex','claude','pi')),
    native_session_id text NOT NULL,
    title text NOT NULL,
    tags text[] NOT NULL DEFAULT '{}',
    recorded_cwd text,
    started_at timestamptz,
    updated_at timestamptz NOT NULL,
    message_count bigint NOT NULL CHECK (message_count >= 0),
    relations jsonb NOT NULL DEFAULT '[]'
);
CREATE INDEX sessions_updated_order ON sessions(updated_at DESC, id DESC);
CREATE INDEX sessions_native ON sessions(source_id, native_key);

CREATE TABLE messages (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence bigint NOT NULL,
    role text NOT NULL,
    native_role text,
    origin text NOT NULL,
    recorded_at timestamptz,
    conversation_parent_id uuid REFERENCES messages(id) DEFERRABLE INITIALLY DEFERRED,
    content jsonb NOT NULL,
    provider_response_id text,
    model text,
    provider_usage jsonb,
    source_refs jsonb NOT NULL DEFAULT '[]',
    UNIQUE (session_id, sequence)
);

CREATE TABLE session_events (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence bigint NOT NULL,
    type text NOT NULL,
    recorded_at timestamptz,
    native_type text NOT NULL,
    data jsonb NOT NULL
);

-- Projects are labels, not access boundaries or an additional lifecycle.
CREATE TABLE projects (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE,
    name text NOT NULL
);
CREATE TABLE session_projects (
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id),
    PRIMARY KEY (session_id, project_id)
);

-- An object is immutable bytes, identified by a unique S3 key.
-- Unreferenced objects are eligible for age-based cleanup; no lifecycle states.
CREATE TABLE objects (
    object_key text PRIMARY KEY,
    sha256 text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE artifacts (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    object_key text NOT NULL REFERENCES objects(object_key),
    media_type text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('image','file','audio','video','other'))
);

-- Collection provenance: clients decide when to start or stop syncing.
CREATE TABLE sources (
    id uuid PRIMARY KEY,
    harness text NOT NULL CHECK (harness IN ('codex','claude','pi')),
    device_label text NOT NULL,
    root text NOT NULL,
    last_synced_at timestamptz,
    UNIQUE (harness, device_label, root)
);
ALTER TABLE sessions ADD FOREIGN KEY (source_id) REFERENCES sources(id);

-- A file is a source-relative path; generations distinguish append from rewrite.
-- Transcript bytes are JSONL. MIME types are needed only for binary artifacts.
CREATE TABLE source_files (
    id uuid PRIMARY KEY,
    source_id uuid NOT NULL REFERENCES sources(id),
    path text NOT NULL,
    generation_id uuid NOT NULL,
    version text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (source_id, path)
);
CREATE TABLE file_generations (
    id uuid PRIMARY KEY,
    file_id uuid NOT NULL REFERENCES source_files(id),
    object_key text NOT NULL REFERENCES objects(object_key),
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE source_files ADD CONSTRAINT source_files_current_generation
    FOREIGN KEY (generation_id) REFERENCES file_generations(id) DEFERRABLE INITIALLY DEFERRED;
CREATE TABLE session_files (
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    file_id uuid NOT NULL REFERENCES source_files(id),
    generation_id uuid NOT NULL REFERENCES file_generations(id),
    PRIMARY KEY (session_id, file_id, generation_id)
);
