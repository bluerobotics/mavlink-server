#!/usr/bin/env python3

import asyncio
import aiohttp
import stat
import sys
import tempfile
from pathlib import Path
from typing import Optional
from utils import handle_output

ARDUSUB_URL = "https://firmware.ardupilot.org/Sub/stable-4.5.3/SITL_x86_64_linux_gnu/ardusub"
BINARY_NAME = "ardusub"

TEMP_DIR = Path(tempfile.gettempdir()) / "ardupilot_firmware"
TEMP_DIR.mkdir(exist_ok=True)
BINARY_PATH = TEMP_DIR / BINARY_NAME

async def download_binary(session: aiohttp.ClientSession) -> None:
    """Download the ArduSub binary."""
    print(f"Downloading {BINARY_NAME}...")

    if BINARY_PATH.exists():
        print(f"{BINARY_NAME} already exists at {BINARY_PATH}")
        return

    async with session.get(ARDUSUB_URL) as response:
        if response.status != 200:
            raise RuntimeError(f"Failed to download binary: {response.status}")

        with open(BINARY_PATH, 'wb') as f:
            async for chunk in response.content.iter_chunked(1024):
                f.write(chunk)

        BINARY_PATH.chmod(BINARY_PATH.stat().st_mode | stat.S_IEXEC)
        print(f"Downloaded {BINARY_NAME} to {BINARY_PATH}")

async def run_sitl() -> Optional[asyncio.Task]:
    async with aiohttp.ClientSession() as session:
        await download_binary(session)

    if not BINARY_PATH.exists():
        print(f"Binary not found at {BINARY_PATH}")
        return None

    try:
        process = await asyncio.create_subprocess_exec(
            str(BINARY_PATH),
            "--model", "vectored",
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE
        )

        return asyncio.create_task(handle_output(process, prefix="SITL"))
    except Exception as e:
        print(f"Failed to start SITL: {e}")
        return None
