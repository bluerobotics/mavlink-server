#!/usr/bin/env python3

import asyncio
import aiohttp
import platform
import sys
from sitl import run_sitl
from server import run_server
from control import run_control

async def main() -> None:
    if platform.system() != "Linux":
        print("This script only works on Linux")
        sys.exit(1)

    if platform.machine() != "x86_64":
        print("This script only works on x86_64 architecture")
        sys.exit(1)

    # Define the components to start in order
    components = [
        ('SITL', run_sitl),
        ('server', run_server),
        ('control', run_control)
    ]

    tasks = []

    # Start each component
    for name, run_func in components:
        print(f"Starting {name}")
        task = await run_func()
        if not task:
            print(f"Failed to start {name}")
            # Cancel all previously started tasks in reverse order
            for t in reversed(tasks):
                t.cancel()
                await t
            sys.exit(1)
        tasks.append(task)

        # Special case: wait after server starts
        if name == 'server':
            await asyncio.sleep(5)

    try:
        # Wait for a while to test to completion or until interrupted
        await asyncio.sleep(20)
    except asyncio.CancelledError:
        print("Received interrupt signal, cleaning up...")
    finally:
        # Clean up all components in reverse order
        print("Stopping components...")

        for task in reversed(tasks):
            task.cancel()
            try:
                await asyncio.wait_for(task, timeout=2.0)
            except (asyncio.TimeoutError, asyncio.CancelledError):
                pass

if __name__ == "__main__":
    try:
        asyncio.run(main())
    except KeyboardInterrupt:
        print("Received keyboard interrupt, exiting...")
        sys.exit(0)
