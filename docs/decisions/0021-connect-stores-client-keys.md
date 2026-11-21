# Save the API key when connecting

Status: accepted. This updates client credential storage in [0013](0013-instance-api-key.md) and [0020](0020-connect-by-url.md).

`sessiondb connect SERVER_URL` verifies the instance key and saves it with the default URL in the client config. The interactive prompt hides input. Scripts can supply `--key-stdin` or `SESSIONDB_API_KEY`. A failed verification preserves the previous connection.

The `[api_keys]` table associates each plaintext key with its normalized server origin. Unix writes use mode `0600`. Background sync reads the key for its pinned server. The CLI redacts keys in config output and omits source lines from config parse errors.

When upgrading a client whose key was stored in an OS credential store, run `connect` once for each server you use. For an installed watcher, stop it during the upgrade, save its key with the updated CLI, then start it again. The instance key on the server continues to come from the server environment.
