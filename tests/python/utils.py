#!/usr/bin/env python3

import asyncio
import sys
from typing import Optional, Callable

async def handle_output(
    process: asyncio.subprocess.Process,
    prefix: str = "",
    on_exit: Optional[Callable[[], None]] = None
) -> None:
    """Handle output from a subprocess with optional prefix and cleanup callback.

    Args:
        process: The subprocess to monitor
        prefix: Optional prefix for output lines
        on_exit: Optional callback to run when process exits
    """
    try:
        while True:
            stdout = await process.stdout.readline()
            stderr = await process.stderr.readline()
            if stdout:
                print(f"{prefix}:\t{stdout.decode().strip()}")
            if stderr:
                print(f"{prefix}:\t{stderr.decode().strip()}", file=sys.stderr)

            if process.returncode is not None:
                print(f"{prefix}Process ended with return code {process.returncode}")
                if on_exit:
                    on_exit()
                break

            await asyncio.sleep(0.1)
    except asyncio.CancelledError:
        print(f"Stopping {prefix.strip()}...")
        process.terminate()
        await process.wait()
        raise
