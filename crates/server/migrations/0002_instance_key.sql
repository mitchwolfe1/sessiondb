-- This is an intentional security-boundary conversion: every former private or
-- organization-isolated record becomes part of one instance-wide collection.
-- Preserve all resource IDs, source provenance, labels, and opaque object keys.
UPDATE sessions
SET document = document - ARRAY['organization_id', 'author_id', 'owner_id', 'visibility'];

-- Dropping these columns also drops the old organization/owner-scoped unique
-- constraints. In particular, do not merge sources with matching provenance or
-- projects with matching slugs: their existing UUIDs and references remain valid.
ALTER TABLE sources
    DROP COLUMN organization_id,
    DROP COLUMN owner_id,
    DROP COLUMN default_visibility;
ALTER TABLE projects
    DROP COLUMN organization_id,
    DROP COLUMN creator_id;
ALTER TABLE imports
    DROP COLUMN organization_id,
    DROP COLUMN requester_id;
ALTER TABLE jobs
    DROP COLUMN organization_id,
    DROP COLUMN requester_id;
ALTER TABLE sessions
    DROP COLUMN organization_id,
    DROP COLUMN owner_id,
    DROP COLUMN visibility;

-- Source UUIDs were already unique across organizations, so these retain the
-- same import-idempotency boundaries without collapsing any existing records.
ALTER TABLE imports ADD UNIQUE (source_id, create_key);
ALTER TABLE imports ADD UNIQUE (source_id, complete_key);
CREATE INDEX sessions_updated_order ON sessions(updated_at DESC, id DESC);

DROP TABLE invitations;
DROP TABLE memberships;
DROP TABLE bootstrap_state;
DROP TABLE organizations;
DROP TABLE users;
