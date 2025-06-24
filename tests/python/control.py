#!/usr/bin/env python3

import asyncio
import math
import sys
from typing import Optional
from pymavlink import mavutil

gps_fix = 0

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

async def check_fix(master: mavutil.mavlink_connection, timeout: float = 1.0):
    while True:
        msg = master.recv_match(type='GPS_RAW_INT', blocking=True, timeout=timeout)
        if msg:
            global gps_fix
            gps_fix = msg.fix_type
        await asyncio.sleep(0.5)

async def disarm_vehicle(master: mavutil.mavlink_connection, timeout: float = 1.0):
    try:
        print("Disarming vehicle...")
        master.arducopter_disarm()
        await asyncio.sleep(timeout)
    except Exception as e:
        print(f"Warning: Could not disarm cleanly: {e}")

async def set_yaw_command(master: mavutil.mavlink_connection, yaw: float):
    master.mav.command_long_send(
        master.target_system,
        master.target_component,
        mavutil.mavlink.MAV_CMD_CONDITION_YAW,
        0,
        yaw,    # param1: target angle (degrees)
        30,     # param2: yaw speed (deg/s)
        1,      # param3: direction (1=cw, -1=ccw)
        0,      # param4: relative (1) or absolute (0)
        0, 0, 0 # unused
    )

async def set_mode(master: mavutil.mavlink_connection, desired_mode: str):
    while True:
        print(f"Setting mode to {desired_mode}")
        master.set_mode_apm(desired_mode)
        await asyncio.sleep(1)
        msg = master.recv_match(type="HEARTBEAT", blocking=True, timeout=1)
        mode = mavutil.mode_string_v10(msg)
        print(f"Waiting for mode({desired_mode}): {mode}")
        if mode.lower() == desired_mode.lower():
            break
        await asyncio.sleep(3)

async def set_yaw(master: mavutil.mavlink_connection, yaw: float, timeout: float = 1.0):
    await set_yaw_command(master, yaw)
    while True:
        attitude = master.recv_match(type='ATTITUDE', blocking=True, timeout=timeout)
        angle = (math.degrees(attitude.yaw) + 360) % 360
        diff = angle - yaw
        print(f"Attitude: {angle:3.2f}, diff: {diff:3.2f}")
        if abs(diff) < 5:
            break
        await asyncio.sleep(1)

async def run_control() -> Optional[asyncio.Task]:
    """Run the vehicle control task."""
    try:
        master = await connect_to_vehicle()
        if not master:
            print("Failed to connect to vehicle")
            return None

        asyncio.ensure_future(check_fix(master))

        async def control_task():
            try:
                armed = await arm_vehicle(master)
                if not armed:
                    print("Failed to arm vehicle within timeout")
                    sys.exit(1)

                while gps_fix == 0:
                    print(f"Waiting for fix: {gps_fix}")
                    await asyncio.sleep(1)
                print("Fix found")

                await set_mode(master, "GUIDED")

                for yaw_angle in [90, 180, 270]:
                    print(f"Set yaw to {yaw_angle} degrees")
                    await set_yaw(master, yaw_angle)

                print("Control test done")
                sys.exit(0)
            except asyncio.CancelledError:
                print("Stopping control...")
                await disarm_vehicle(master)
                raise

        return asyncio.create_task(control_task())
    except Exception as e:
        print(f"Failed to start control: {e}")
        return None
