CREATE TABLE users (
    id uuid PRIMARY KEY,
    issuer text NOT NULL,
    subject text NOT NULL,
    display_name text NOT NULL,
    email text,
    UNIQUE (issuer, subject)
);
CREATE TABLE organizations (
    id uuid PRIMARY KEY,
    slug text NOT NULL UNIQUE,
    name text NOT NULL
);
CREATE TABLE memberships (
    organization_id uuid NOT NULL REFERENCES organizations(id),
    user_id uuid NOT NULL REFERENCES users(id),
    role text NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'disabled')),
    PRIMARY KEY (organization_id, user_id)
);
CREATE TABLE invitations (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    email text NOT NULL,
    role text NOT NULL CHECK (role IN ('admin', 'member')),
    expires_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','accepted','revoked','expired'))
);
CREATE UNIQUE INDEX invitation_pending_email ON invitations(organization_id, lower(email)) WHERE status = 'pending';
CREATE TABLE bootstrap_state (singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton), completed_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE sources (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    owner_id uuid NOT NULL REFERENCES users(id),
    harness text NOT NULL CHECK (harness IN ('codex','claude','pi')),
    device_label text NOT NULL,
    root text NOT NULL,
    default_visibility text NOT NULL DEFAULT 'private' CHECK (default_visibility IN ('private','organization')),
    paused boolean NOT NULL DEFAULT false,
    last_synced_at timestamptz,
    UNIQUE (organization_id, owner_id, device_label, harness, root)
);
CREATE INDEX sources_owner ON sources(organization_id, owner_id, id);
CREATE TABLE source_files (
    id uuid PRIMARY KEY,
    source_id uuid NOT NULL REFERENCES sources(id),
    path text NOT NULL,
    generation_id uuid NOT NULL,
    version text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    sha256 text NOT NULL,
    media_type text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (source_id, path)
);
CREATE TABLE file_generations (
    id uuid PRIMARY KEY,
    file_id uuid NOT NULL REFERENCES source_files(id),
    object_key text NOT NULL UNIQUE,
    byte_length bigint NOT NULL CHECK (byte_length >= 0),
    sha256 text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE source_files ADD CONSTRAINT source_files_current_generation FOREIGN KEY (generation_id) REFERENCES file_generations(id) DEFERRABLE INITIALLY DEFERRED;
CREATE TABLE projects (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    creator_id uuid NOT NULL REFERENCES users(id),
    slug text NOT NULL,
    name text NOT NULL,
    archived boolean NOT NULL DEFAULT false,
    UNIQUE (organization_id, slug)
);
CREATE TABLE imports (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    source_id uuid NOT NULL REFERENCES sources(id),
    requester_id uuid NOT NULL REFERENCES users(id),
    manifest jsonb NOT NULL,
    request_hash text NOT NULL,
    create_key text,
    complete_key text,
    status text NOT NULL DEFAULT 'awaiting_upload' CHECK (status IN ('awaiting_upload','verifying','queued','processing','ready','partial','failed','expired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    upload_expires_at timestamptz NOT NULL DEFAULT now() + interval '24 hours',
    accepted_at timestamptz,
    closed_at timestamptz,
    job_id uuid,
    results jsonb NOT NULL DEFAULT '[]',
    errors jsonb NOT NULL DEFAULT '[]',
    UNIQUE (organization_id, source_id, create_key),
    UNIQUE (organization_id, source_id, complete_key)
);
CREATE TABLE import_files (
    import_id uuid NOT NULL REFERENCES imports(id),
    file_id uuid NOT NULL REFERENCES source_files(id),
    generation_id uuid NOT NULL REFERENCES file_generations(id),
    version text NOT NULL,
    object_key text NOT NULL,
    path text NOT NULL,
    PRIMARY KEY (import_id, file_id)
);
CREATE TABLE objects (
    object_key text PRIMARY KEY,
    import_id uuid REFERENCES imports(id),
    state text NOT NULL CHECK (state IN ('staging','promotion','accepted','retired')),
    created_at timestamptz NOT NULL DEFAULT now(),
    retired_at timestamptz
);
CREATE INDEX objects_cleanup ON objects(state, created_at, retired_at);
CREATE TABLE jobs (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    requester_id uuid NOT NULL REFERENCES users(id),
    source_id uuid NOT NULL REFERENCES sources(id),
    import_id uuid NOT NULL REFERENCES imports(id),
    kind text NOT NULL CHECK (kind IN ('verify_import','parse')),
    status text NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','succeeded','partial','failed','cancelled')),
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    lease_owner uuid,
    lease_until timestamptz,
    completed_items bigint NOT NULL DEFAULT 0,
    total_items bigint,
    error_code text,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (import_id, kind)
);
CREATE INDEX jobs_claim ON jobs(available_at, created_at) WHERE status IN ('queued','running');
ALTER TABLE imports ADD CONSTRAINT imports_job FOREIGN KEY (job_id) REFERENCES jobs(id);
CREATE TABLE sessions (
    id uuid PRIMARY KEY,
    organization_id uuid NOT NULL REFERENCES organizations(id),
    source_id uuid NOT NULL REFERENCES sources(id),
    owner_id uuid NOT NULL REFERENCES users(id),
    native_key text NOT NULL,
    harness text NOT NULL,
    native_session_id text NOT NULL,
    visibility text NOT NULL CHECK (visibility IN ('private','organization')),
    revision text NOT NULL,
    updated_at timestamptz NOT NULL,
    document jsonb NOT NULL
);
CREATE INDEX sessions_access_order ON sessions(organization_id, updated_at DESC, id DESC);
CREATE INDEX sessions_native ON sessions(source_id, native_key);
CREATE TABLE session_projects (
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    project_id uuid NOT NULL REFERENCES projects(id),
    PRIMARY KEY (session_id, project_id)
);
CREATE TABLE session_files (
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    file_id uuid NOT NULL REFERENCES source_files(id),
    generation_id uuid NOT NULL REFERENCES file_generations(id),
    PRIMARY KEY (session_id, file_id, generation_id)
);
CREATE TABLE messages (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence bigint NOT NULL,
    document jsonb NOT NULL,
    full_content jsonb NOT NULL,
    UNIQUE (session_id, sequence)
);
CREATE TABLE session_events (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    sequence bigint NOT NULL,
    document jsonb NOT NULL
);
CREATE TABLE artifacts (
    id uuid PRIMARY KEY,
    session_id uuid NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    object_key text NOT NULL UNIQUE,
    media_type text NOT NULL,
    kind text NOT NULL,
    byte_length bigint NOT NULL CHECK (byte_length >= 0)
);
