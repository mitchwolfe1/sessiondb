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

    server.run()


if __name__ == "__main__":
    main()
