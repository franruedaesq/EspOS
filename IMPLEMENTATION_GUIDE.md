# EspOS Implementation Guide

**Project**: EspOS v0.1.0 — Embassy async OS for ESP32-S3
**Hardware**: ESP32-S3 (QFN56, revision v0.2, 16 MB flash, 8 MB PSRAM)
**Host**: macOS (Apple Silicon)

---

## Project Structure

```
EspOS/
├── src/
│   ├── main.rs               # Entry point, task spawning
│   ├── state_machine.rs      # Core orchestrator
│   ├── memory.rs             # Heap management
│   ├── tasks/
│   │   ├── heartbeat.rs      # LED blink (1 Hz)
│   │   ├── telemetry.rs      # RAM/CPU monitoring
│   │   ├── wifi.rs           # WiFi + DHCP
│   │   ├── web_server.rs     # HTTP dashboard (port 80)
│   │   ├── imu.rs            # MPU6500 sensor fusion
│   │   ├── motor.rs          # PWM motor control
│   │   ├── can_bus.rs        # TWAI/CAN bus
│   │   └── ui.rs             # Display (disabled)
│   └── drivers/              # SSD1306, ST7789, MPU6500, VL53L0X, L298N
├── patches/esp-hal-embassy/  # Local patch for low_power_wait fix
├── .cargo/config.toml        # Build + flash runner config
└── Cargo.toml
```

---

## Prerequisites

```bash
# Xtensa Rust toolchain
cargo install espup && espup install

# Flash tool — must be 3.x, NOT 4.x (see Flashing section)
cargo install espflash --version "^3"
```

Add to `~/.zshrc` so the toolchain is always available:
```bash
. $HOME/export-esp.sh
```

---

## Build Configuration

### `.cargo/config.toml`

```toml
[target.xtensa-esp32s3-none-elf]
runner = "espflash flash --monitor -p /dev/cu.usbmodem5B5F1229581"
rustflags = [
    "-C", "link-arg=-nostartfiles",
    "-C", "link-arg=-Tlinkall.x",
]

[build]
target = "xtensa-esp32s3-none-elf"

[unstable]
build-std = ["core", "alloc"]

[env]
ESP_HAL_EMBASSY_CONFIG_LOW_POWER_WAIT = "false"
WIFI_SSID = "YourSSID"
WIFI_PASSWORD = "YourPassword"
```

> ⚠️ WiFi credentials are compiled into the binary. Never commit this file to a public repo.

### WiFi Requirements
- 2.4 GHz only (ESP32 does not support 5 GHz)
- WPA2-PSK, DHCP enabled

---

## Build & Flash

### First Flash (or after a bad bootloader)

```bash
# 1. Erase chip (required if espflash 4.x was ever used — it installs an incompatible bootloader)
espflash erase-flash -p /dev/cu.usbmodem5B5F1229581

# 2. Build and flash
cargo run --release
```

### Normal Workflow

```bash
cargo run --release
```

This builds, flashes, and opens the serial monitor automatically.

---

## Boot Output

```
=== EspOS Starting ===
=== HAL initialized ===
=== Heap initialized ===
=== Embassy timer initialized ===
=== Hardware watchdog disabled ===
=== Heartbeat task spawned ===
=== WiFi initialized, spawning tasks ===
=== WiFi and web server tasks spawned ===
=== All tasks spawned, entering state machine ===
[wifi] connecting to 'YourSSID'…
[wifi] IP: 192.168.x.x       ← copy this
[web_server] HTTP server listening on port 80
```

Open `http://<IP>/` in a browser for the live dashboard.

---

## Web Dashboard

**Endpoints:**
- `GET /` — HTML dashboard (auto-refreshes every 1 s)
- `GET /api/stats` — JSON telemetry

**Metrics:** WiFi status + IP, uptime, SRAM used/free, CPU load %, heartbeat health

---

## Known Issues & Fixes

### espflash 4.x — incompatible bootloader

`espflash` 4.x bundles an ESP-IDF v5.5.1 bootloader that fails on bare-metal `esp-hal` apps:

```
E boot_comm: Image requires efuse blk rev >= v193.87, but chip is v1.3
```

**Fix:** Use `espflash` 3.x and erase flash before the first use.

```bash
cargo install espflash --version "^3"
espflash erase-flash -p /dev/cu.usbmodem5B5F1229581
```

### `--ignore-app-descriptor` flag removed in 3.x

This flag only exists in 4.x. It is not needed with 3.x.

### Linker not found

```bash
. $HOME/export-esp.sh   # must run in every new terminal
```

---

## Next Steps

- Enable PSRAM (`memory::init_psram!()`)
- Re-enable SSD1306 display and joystick UI
- OTA firmware updates
- WebSocket for real-time push instead of polling
- NVS for persistent config (WiFi credentials at runtime)
