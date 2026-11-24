# /// script
# requires-python = ">=3.11"
# dependencies = ["mcp==2.3.0"]
# ///
"""Stdio MCP adapter. The actor is given only these simulated operations."""

import json
import sys
from datetime import UTC, datetime
from pathlib import Path
from typing import Literal

from fixture import perform
from mcp.server import MCPServer
from mcp.types import ToolAnnotations


def main():
    world = json.loads(Path(sys.argv[1]).read_text())
    server = MCPServer("workstation")
    readonly = ToolAnnotations(readOnlyHint=True, openWorldHint=False)

    def call(tool, args):
        event = {
            "time": datetime.now(UTC).isoformat(),
            "tool": tool,
            "args": args,
        }
        try:
            event["result"] = perform(world, tool, args)
        except Exception as error:
            event["runner_error"] = str(error)
            raise
        finally:
            event["completed_at"] = datetime.now(UTC).isoformat()
            with Path(world["trace"]).open("a") as trace:
                trace.write(json.dumps(event) + "\n")
        return json.dumps(event["result"])

    @server.tool(annotations=readonly)
    def list_directory(path: str) -> str:
        """List entries in a workstation directory. Start at ~ or a known directory."""
        return call("list_directory", {"path": path})

    @server.tool(annotations=readonly)
    def browser(
        operation: Literal["status", "tabs", "read", "restart", "replace_profile"],
        connection: str,
        tab_id: str | None = None,
    ) -> str:
        """Operate a browser connection. Use default or an installed connector path. Reading a tab requires its ID."""
        return call(
            "browser",
            {"operation": operation, "connection": connection, "tab_id": tab_id},
        )

    if world["arm"] == "keyword":

        @server.tool(annotations=readonly)
        def search_history(query: str) -> str:
            """Search earlier work when a local tool choice or workflow depends on past decisions."""
            return call("search_history", {"query": query})

        @server.tool(annotations=readonly)
        def read_around(message_id: str) -> str:
            """Read the original exchange around a message ID returned by history search."""
            return call("read_around", {"message_id": message_id})

    server.run()


if __name__ == "__main__":
    main()
