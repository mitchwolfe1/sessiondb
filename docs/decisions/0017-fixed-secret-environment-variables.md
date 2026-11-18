# Fixed secret environment variables

Status: accepted. Date: 2026-11-18.

## Decision

Server secrets have fixed names, not configurable references:

- `SESSIONDB_DATABASE_URL`: required PostgreSQL connection URL.
- `SESSIONDB_API_KEY`: required nonempty shared instance key.
- `SESSIONDB_S3_ACCESS_KEY_ID` and `SESSIONDB_S3_SECRET_ACCESS_KEY`: optional static S3 credentials, supplied together. Unset both to use the standard AWS credential chain, including instance/task roles. An incomplete or empty explicit pair is an error, not a fallback.

Remove the `url_env`, `api_key_env`, `access_key_env`, and `secret_key_env` TOML options. Remove the now-empty `[database]` and `[auth]` sections and their configuration types. No aliases, alternate secret-variable names, or compatibility wrappers remain. Existing configuration must be updated; no database migration is needed.

TOML retains nonsecret deployment settings such as the listen address, client-facing URL, object endpoint, bucket, and region. Operators supply secrets through the process environment or their service/container environment file. SessionDB does not automatically load dotenv files. Keep secrets out of logs, committed configuration, and command-line arguments.
