#!/usr/bin/env bash

set -e

TARGET=armv7-unknown-linux-gnueabihf
BUILDTYPE=release

cross build --$BUILDTYPE --target=$TARGET

/home/joaoantoniocardoso/BlueRobotics/cross_build_dev/old/upload_to_blueos.sh \
    target/$TARGET/$BUILDTYPE/mavlink-server \
    /home/pi/mavlink-server

echo ""
echo ""
echo 'clear; sshpass -p raspberry scp -o StrictHostKeyChecking=no pi@localhost:/home/pi/mavlink-server "$(which mavlink-server)" ; /home/pi/services/ardupilot_manager/main.py'
echo ""
echo ""
