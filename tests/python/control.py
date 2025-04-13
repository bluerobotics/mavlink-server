#!/usr/bin/env python3

import asyncio
import sys
from typing import Optional
from mavsdk import System

async def connect_to_vehicle(timeout: float = 3.0) -> Optional[System]:
    """Connect to the vehicle via MAVSDK with a timeout.

    Args:
        timeout: Maximum time to wait for connection in seconds
    """
    drone = System()
    print("Connecting to drone...")
    try:
        await asyncio.wait_for(drone.connect(system_address="udp://:14660"), timeout=3.0)
    except asyncio.TimeoutError:
        print("Timeout while attempting to connect")
        return None

    print("Waiting for drone to connect...")
    start_time = asyncio.get_event_loop().time()

    async for state in drone.core.connection_state():
        if state.is_connected:
            print("Drone connected!")
            return drone

        if asyncio.get_event_loop().time() - start_time > timeout:
            print("Timeout waiting for drone to connect")
            return None

        await asyncio.sleep(0.1)

    return None

async def arm_vehicle(drone: System, timeout: float = 5.0) -> bool:
    """ Arm the vehicle with a timeout.

    Args:
        drone: The drone object
        timeout: Maximum time to wait for arming in seconds
    Returns:
        True if the vehicle was armed successfully, False otherwise
    """
    try:
        print("Arming vehicle...")
        await drone.action.arm()

        # Wait for armed state with timeout
        armed = False
        start_time = asyncio.get_event_loop().time()

        async for is_armed in drone.telemetry.armed():
            if is_armed:
                print("Vehicle armed successfully!")
                armed = True
                break

            if asyncio.get_event_loop().time() - start_time > timeout:
                print("Timeout waiting for vehicle to arm")
                break

            await asyncio.sleep(0.1)

        return armed
    except Exception as e:
        print(f"Failed to arm vehicle: {e}")
        return False

async def run_control() -> Optional[asyncio.Task]:
    """Run the vehicle control task."""
    try:
        drone = await connect_to_vehicle()
        if not drone:
            print("Failed to connect to vehicle")
            return None

        async def control_task():
            try:
                armed = await arm_vehicle(drone)
                if not armed:
                    print("Failed to arm vehicle within timeout")
                    sys.exit(1)

                # Keep the task running to maintain the connection
                while True:
                    await asyncio.sleep(1)
            except asyncio.CancelledError:
                print("Stopping control...")
                try:
                    # Try to disarm, but don't wait too long
                    await asyncio.wait_for(drone.action.disarm(), timeout=1.0)
                except (asyncio.TimeoutError, Exception) as e:
                    print(f"Warning: Could not disarm cleanly: {e}")
                raise

        return asyncio.create_task(control_task())
    except Exception as e:
        print(f"Failed to start control: {e}")
        return None
