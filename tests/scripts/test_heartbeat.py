import asyncio
import sys
from pymavlink import mavutil

async def send_heartbeat(conn_str):
    conn = mavutil.mavlink_connection(conn_str, source_system=33, source_component=33)
    try:
        while True:
            print("Sending heartbeat")
            conn.mav.heartbeat_send(
                0,
                mavutil.mavlink.MAV_TYPE_GENERIC,
                mavutil.mavlink.MAV_AUTOPILOT_GENERIC,
                0,
                mavutil.mavlink.MAV_STATE_ACTIVE,
                3
            )
            await asyncio.sleep(1)
    except Exception as error:
        print(f"Error sending heartbeat: {error}")
    finally:
        print("Heartbeat task finished")
        conn.close()

async def wait_for_heartbeat(conn_str, timeout=10):
    conn = mavutil.mavlink_connection(conn_str)
    try:
        start_time = asyncio.get_event_loop().time()

        while True:
            remaining_time = timeout - (asyncio.get_event_loop().time() - start_time)
            if remaining_time <= 0:
                print("No heartbeat received within timeout.")
                return 1
            try:
                msg = await asyncio.wait_for(asyncio.to_thread(conn.wait_heartbeat), remaining_time)
                print(f"Heartbeat received from system {msg.get_srcSystem()}:{msg.get_srcComponent()}")

                if msg.get_srcSystem() == 33 and msg.get_srcComponent() == 33:
                    print("Received matching heartbeat, test passed!")
                    return 0

            except Exception as error:
                print(f"Error receiving heartbeat: {error}")
                return 1

    except Exception as error:
        print(f"Error waiting for heartbeat: {error}")
        return 1
    finally:
        print("Waiting for heartbeat task finished")
        conn.close()

async def main(server_endpoint, client_endpoint):
    send_task = asyncio.create_task(send_heartbeat(client_endpoint))
    try:
        result = await asyncio.wait_for(wait_for_heartbeat(server_endpoint), timeout=10)
        return result
    except asyncio.TimeoutError:
        print("Test timed out")
        return 1
    finally:
        send_task.cancel()
        try:
            await send_task
        except asyncio.CancelledError:
            pass

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: python test_mavlink_heartbeat.py endpoint1 endpoints2")
        sys.exit(1)
    try:
        result = asyncio.run(asyncio.wait_for(main(sys.argv[1], sys.argv[2]), timeout=15))
        sys.exit(result)
    except asyncio.TimeoutError:
        print("Script timed out")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
