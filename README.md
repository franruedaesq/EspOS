# EspOS

> **Modular embedded Rust OS for the ESP32-S3, built on the Embassy async framework.**

EspOS is a real-time, agentic operating system for the ESP32-S3 microcontroller. It provides a structured, async-first foundation for autonomous rover applications: voice-command processing via an LLM API over WiFi, sensor fusion from an inertial measurement unit (IMU) and a time-of-flight (ToF) distance sensor, differential-drive motor control, a TFT display UI, a CAN bus message router, and a serial command-line interface — all running concurrently on the dual-core Xtensa processor.

---

## Table of Contents

- [What Is EspOS?](#what-is-espos)
- [What Can It Do?](#what-can-it-do)
- [Hardware Requirements](#hardware-requirements)
- [Getting Started](#getting-started)
- [Project Structure](#project-structure)
- [Configuration](#configuration)
- [Architecture](#architecture)
- [License](#license)

---

## What Is EspOS?

EspOS is a `no_std` / `no_main` embedded Rust application targeting the **ESP32-S3** SoC. It is designed around three ideas:

1. **Agentic workflow** — a state machine orchestrates the full sense-think-act loop: capture audio → call an LLM API over WiFi → execute the returned motion command → report telemetry.
2. **Modular HAL** — hardware-specific code lives behind trait objects (`RoverChassis`, `SpatialSensor`, `AudioInput`/`AudioOutput`), making it straightforward to swap drivers or port to new hardware.
3. **Async concurrency** — every subsystem runs as an independent [Embassy](https://embassy.dev/) task, communicating through lock-free static channels rather than shared mutable state.

---

## What Can It Do?

| Capability | Details |
|---|---|
| **Agentic state machine** | `Idle → Listening → Processing → Moving → Reporting` loop with emergency-stop override via CAN collision alerts |
| **Voice command (LLM)** | Captures 1 s of audio at 16 kHz/16-bit mono, POSTs it to an LLM endpoint over WiFi, parses the returned motion command |
| **WiFi + DHCP** | Connects to a WPA2 access point, obtains an IP address, and maintains the link with auto-reconnect |
| **IMU sensor fusion** | Reads the MPU-6500 at 100 Hz; a complementary filter (α = 0.98) blends gyroscope integration and accelerometer tilt to produce roll and pitch angles |
| **Time-of-flight ranging** | Reads the VL53L0X distance sensor for obstacle detection (0–2 000 mm) |
| **Differential-drive control** | Commands the L298N dual H-bridge over PWM; supports forward, backward, point-turn, brake, and coast modes |
| **CAN / TWAI bus** | Initialises the ESP32-S3 TWAI peripheral at 500 kbps; routes frames by ID range (collision alerts, sensor data, telemetry, config) |
| **TFT display (240 × 240)** | Renders state name, status text, and progress bars on an ST7789 panel via SPI using embedded-graphics at ~30 fps |
| **Serial CLI** | 115 200-baud command-line interface on UART0; accepts `help` and `status` commands |
| **Real-time telemetry** | Logs heap usage, CPU load, and battery voltage as JSON at 1 Hz |
| **Hardware watchdog** | TIMG1 MWDT with a 10 s timeout, fed every second by the heartbeat task; reboots the MCU on hang |
| **Dual-core scheduling** | Core 0: WiFi, CAN, UI, CLI, telemetry, state machine. Core 1: IMU fusion, motor control |
| **Multi-region heap** | 512 KB internal SRAM + 8 MB octal PSRAM, managed by `esp-alloc` |

### Supported Peripherals

| Peripheral | Interface | Role |
|---|---|---|
| MPU-6500 | I²C | 6-DoF IMU (accelerometer + gyroscope) |
| VL53L0X | I²C | Time-of-flight distance sensor |
| L298N | GPIO + PWM (LEDC) | Dual H-bridge motor driver |
| ST7789 | SPI | 240 × 240 TFT colour display |
| INMP441 | I²S RX | MEMS microphone (trait defined, driver stub) |
| MAX98357A | I²S TX | Class-D audio amplifier (trait defined, driver stub) |
| SN65HVD230 | TWAI/CAN | CAN bus transceiver |

---

## Hardware Requirements

- **MCU**: ESP32-S3 (dual-core Xtensa LX7, 240 MHz)
- **RAM**: 512 KB SRAM built-in; 8 MB PSRAM (OCTAL) recommended for the display framebuffer and audio buffers
- **Flash**: ≥ 4 MB
- **Peripherals**: see the table above for the full sensor/actuator list

---

## Getting Started

### Prerequisites

1. Install the Rust toolchain (the correct nightly version is pinned in `rust-toolchain.toml`):
   ```sh
   rustup toolchain install nightly
   rustup target add xtensa-esp32s3-none-elf
   ```
2. Install [espflash](https://github.com/esp-rs/espflash):
   ```sh
   cargo install espflash
   ```
3. Install [cargo-espflash](https://github.com/esp-rs/espflash) or use `espflash` directly.

### Build

```sh
cargo build --release
```

### Flash

```sh
espflash flash --release --monitor
```

### Monitor

```sh
espflash monitor
```

Serial output appears on UART0 at 115 200 baud. Type `help` in the serial monitor to list CLI commands.

---

## Project Structure

```
EspOS/
├── Cargo.toml            # Crate manifest and dependency versions
├── rust-toolchain.toml   # Pinned Rust/Xtensa toolchain
├── build.rs              # Build script (linker script selection)
├── README.md             # This file
├── ARCHITECTURE.md       # Detailed architecture documentation
└── src/
    ├── main.rs           # Entry point: hardware init, task spawning, state-machine launch
    ├── memory.rs         # Heap allocator setup (SRAM + PSRAM regions)
    ├── state_machine.rs  # Agentic rover orchestrator (state enum + event loop)
    ├── hal/
    │   ├── mod.rs        # Re-exports
    │   ├── chassis.rs    # RoverChassis trait + L298nChassis implementation
    │   ├── sensor.rs     # SpatialSensor trait + Vector3D, CombinedSpatialSensor
    │   └── audio.rs      # AudioInput / AudioOutput traits + I2sError
    ├── drivers/
    │   ├── mod.rs        # Re-exports
    │   ├── mpu6500.rs    # MPU-6500 I²C driver (accel + gyro)
    │   ├── vl53l0x.rs    # VL53L0X I²C driver (ToF range)
    │   ├── l298n.rs      # L298N GPIO/PWM driver (H-bridge)
    │   └── st7789.rs     # ST7789 SPI driver (TFT display)
    └── tasks/
        ├── mod.rs        # Re-exports
        ├── heartbeat.rs  # 1 Hz LED blink + watchdog feed
        ├── imu.rs        # 100 Hz sensor fusion (complementary filter)
        ├── motor.rs      # Motor command queue + safety timeout
        ├── wifi.rs       # WiFi association, DHCP, reconnect
        ├── can_bus.rs    # TWAI init, frame routing, priority channels
        ├── ui.rs         # ST7789 framebuffer + embedded-graphics draw loop
        ├── telemetry.rs  # 1 Hz JSON health metrics (RAM, CPU, battery)
        └── cli.rs        # Serial command-line interface
```

---

## Configuration

Key build-time constants (edit the relevant source files before flashing):

| Constant | Location | Default | Description |
|---|---|---|---|
| WiFi SSID / password | `src/tasks/wifi.rs` | `""` | Access-point credentials |
| LLM API endpoint | `src/state_machine.rs` | *(placeholder)* | URL for the voice-command LLM |
| IMU filter alpha | `src/tasks/imu.rs` | `0.98` | Complementary filter gyro weight |
| Watchdog timeout | `src/main.rs` | 10 s | MWDT timeout before reset |
| Motor safety timeout | `src/tasks/motor.rs` | 5 s | Auto-stop if no command received |
| CAN bus speed | `src/tasks/can_bus.rs` | 500 kbps | TWAI baud rate |
| Display FPS | `src/tasks/ui.rs` | 30 fps | Target frame rate |
| Telemetry interval | `src/tasks/telemetry.rs` | 1 Hz | JSON log cadence |

---

## Architecture

See [ARCHITECTURE.md](ARCHITECTURE.md) for a detailed description of the software layers, task model, inter-task communication channels, the agentic state machine, and the memory layout.

---

## License

This project is licensed under the [MIT License](LICENSE).
