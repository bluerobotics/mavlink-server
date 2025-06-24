#!/usr/bin/env python3

import asyncio
import sys
from typing import Optional
from pymavlink import mavutil

async def connect_to_vehicle(timeout: float = 3.0) -> Optional[mavutil.mavlink_connection]:
    """Connect to the vehicle via pymavlink with a timeout."""
    print("Connecting to drone...")
    try:
        master = mavutil.mavlink_connection("udp:localhost:14660", autoreconnect=True)
        master.wait_heartbeat(timeout=timeout)
        print("Drone connected!")
        return master
    except Exception as e:
        print(f"Timeout while attempting to connect: {e}")
        return None

async def arm_vehicle(master: mavutil.mavlink_connection, timeout: float = 5.0) -> bool:
    """Arm the vehicle with a timeout."""
    try:
        print("Arming vehicle...")
        master.arducopter_arm()

        start = asyncio.get_event_loop().time()
        while True:
            master.recv_match(type="HEARTBEAT", blocking=False)
            if master.motors_armed():
                print("Vehicle armed successfully!")
                return True
            if asyncio.get_event_loop().time() - start > timeout:
                print("Timeout waiting for vehicle to arm")
                return False
            await asyncio.sleep(0.1)
    except Exception as e:
        print(f"Failed to arm vehicle: {e}")
        return False

async def disarm_vehicle(master: mavutil.mavlink_connection, timeout: float = 1.0):
    try:
        print("Disarming vehicle...")
        master.arducopter_disarm()
        await asyncio.sleep(timeout)
    except Exception as e:
        print(f"Warning: Could not disarm cleanly: {e}")

async def run_control() -> Optional[asyncio.Task]:
    """Run the vehicle control task."""
    try:
        master = await connect_to_vehicle()
        if not master:
            print("Failed to connect to vehicle")
            return None

        async def control_task():
            try:
                armed = await arm_vehicle(master)
                if not armed:
                    print("Failed to arm vehicle within timeout")
                    sys.exit(1)

                while True:
                    await asyncio.sleep(1)
            except asyncio.CancelledError:
                print("Stopping control...")
                await disarm_vehicle(master)
                raise

        return asyncio.create_task(control_task())
    except Exception as e:
        print(f"Failed to start control: {e}")
        return None
