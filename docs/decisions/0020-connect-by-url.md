# Connect by server URL

Status: accepted. This replaces named CLI contexts in [0013](0013-instance-api-key.md) and [0016](0016-http-private-networks.md).

`sessiondb connect SERVER_URL` authenticates to the supplied HTTP or HTTPS origin and saves its normalized URL after a successful info request. The URL identifies the destination. `config show` displays the effective server. Instance credentials continue to use the environment or the OS credential store keyed by origin.

Each selected native root belongs to a server URL. A watcher pins that URL at installation and reloads its roots from the client configuration. Connecting to a different default affects future CLI invocations. To retarget an installed watcher, enable roots for the new server and run `sync install` for that destination.

## Upgrading an existing client

The client reads old configuration files, resolves each source’s former name to its server, and coalesces identical registrations for the same server. Previously registered URLs can recover destinations for older configs created with a one-off server override. A config write saves the URL-based format. If a legacy entry has no recoverable destination, supply its original URL in that source entry.

After replacing the CLI binary, refresh a running login service with `sessiondb --server ORIGINAL_URL sync install` before saving the new configuration. This relaunches the worker against the original destination. Existing generated watcher arguments also work when the service manager launches the upgraded binary. Client receipts, pending snapshots, and server credentials retain their existing identities.
