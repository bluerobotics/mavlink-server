# MAVLink-Server

<div align="center">

![](./docs/preview.png)

</div>
<br>


<h4 align="center">
   A MALink service that allows drivers to implement features based on an internal MAVLink bus.<br>
   Currently, it provide router and broker features.
</h4>

[![Test and Build](https://github.com/bluerobotics/mavlink-server/actions/workflows/build.yml/badge.svg)](https://github.com/bluerobotics/mavlink-server/actions/workflows/build.yml)

## Features
- Web Interface via default web server ([0.0.0.0:8080](0.0.0.0:8080))
- Software and Vehicle control via REST API

#### Supported Endpoints
- TLog
- Serial
- TCP
- UDP
- Fake
- Zenoh
- WebSocket
- Rest (Compatible with [mavlink2rest](https://github.com/mavlink/mavlink2rest))

## Installation

Download the binaries available on the [latest release](https://github.com/bluerobotics/mavlink-server/releases/latest).
