#!/usr/bin/env python3

import asyncio
import os
import sys
from typing import Optional
from utils import handle_output

async def run_server() -> Optional[asyncio.Task]:
    """Run the Rust server binary with specified arguments."""
    server_args = [
        "tcpclient:0.0.0.0:5760",
        "udpout:0.0.0.0:14660",
        "--verbose",
    ]

    binary = os.environ.get("MAVLINK_SERVER_BIN")
    if binary:
        command = [binary, *server_args]
    else:
        command = ["cargo", "run", "--", *server_args]

    try:
        process = await asyncio.create_subprocess_exec(
            *command,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE
        )

        return asyncio.create_task(handle_output(process, prefix="SERVER"))
    except Exception as e:
        print(f"Failed to start server: {e}")
        return None

async def stop_server(task: asyncio.Task) -> None:
    """Stop the server task and wait for cleanup."""
    task.cancel()
    try:
        await task
    except asyncio.CancelledError:
        pass
