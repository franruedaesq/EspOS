# EspOS Architecture

This document describes the internal design of EspOS: its software layers, the Embassy task model, inter-task communication, the agentic state machine, and the memory layout.

---

## Table of Contents

- [Overview](#overview)
- [Software Layers](#software-layers)
- [Boot Sequence](#boot-sequence)
- [Task Model](#task-model)
- [Inter-Task Communication](#inter-task-communication)
- [Agentic State Machine](#agentic-state-machine)
- [Hardware Abstraction Layer (HAL)](#hardware-abstraction-layer-hal)
- [Device Drivers](#device-drivers)
- [Memory Layout](#memory-layout)
- [CAN Bus Message Routing](#can-bus-message-routing)
- [Display Pipeline](#display-pipeline)

---

## Overview

EspOS is structured as a set of concurrent Embassy async tasks that communicate through statically allocated, lock-free channels. No task owns another; all coordination happens through message passing. A top-level state machine (running on the Core 0 main task) orchestrates the high-level sense-think-act loop, while specialised tasks handle sensors, motors, networking, and the UI independently.

```
┌─────────────────────────────────────────────────────────────┐
│                        Application                          │
│               Agentic State Machine (Core 0)                │
└──────────┬────────────────────────────────┬─────────────────┘
           │ channels                        │ channels
┌──────────▼─────────────────────────────────▼─────────────────┐
│                    Embassy Async Tasks                        │
│  Core 0: wifi · can_bus · ui · telemetry · cli · heartbeat   │
│  Core 1: imu · motor                                         │
└──────────┬─────────────────────────────────┬─────────────────┘
           │ traits                           │ traits
┌──────────▼──────────────┐   ┌──────────────▼─────────────────┐
│  Hardware Abstraction   │   │      Device Drivers             │
│  Layer (HAL)            │   │  mpu6500 · vl53l0x · l298n ·   │
│  chassis · sensor ·     │   │  st7789                         │
│  audio                  │   │                                 │
└──────────┬──────────────┘   └──────────────┬─────────────────┘
           │                                  │
┌──────────▼──────────────────────────────────▼─────────────────┐
│                        esp-hal / ESP32-S3                      │
│         I²C · SPI · LEDC/PWM · TWAI · WiFi · I²S · UART       │
└────────────────────────────────────────────────────────────────┘
```

---

## Software Layers

| Layer | Location | Responsibility |
|---|---|---|
| **Application** | `src/state_machine.rs` | Agentic sense-think-act loop |
| **Tasks** | `src/tasks/` | One Embassy task per subsystem |
| **HAL** | `src/hal/` | Trait abstractions for chassis, sensors, audio |
| **Drivers** | `src/drivers/` | Concrete peripheral implementations |
| **Memory** | `src/memory.rs` | Multi-region heap initialisation |
| **Runtime** | `esp-hal`, `esp-hal-embassy` | Peripheral clocks, interrupt routing, async executor |

---

## Boot Sequence

```
main() [Core 0]
  │
  ├─ esp_hal::init()           // configure clocks, GPIO matrix
  ├─ memory::init_heap()       // register 512 KB SRAM heap
  ├─ memory::init_psram!()     // register 8 MB PSRAM heap (if available)
  ├─ embassy init (TIMG0)      // start Embassy time-keeper
  ├─ MWDT init (TIMG1)         // arm 10 s hardware watchdog
  │
  ├─ spawn heartbeat_task      // Core 0 – 1 Hz LED + watchdog feed
  ├─ spawn wifi_task           // Core 0 – association + DHCP
  ├─ spawn can_bus_task        // Core 0 – TWAI RX/TX router
  ├─ spawn ui_task             // Core 0 – display framebuffer at ~30 fps
  ├─ spawn telemetry_task      // Core 0 – 1 Hz JSON health log
  ├─ spawn cli_task            // Core 0 – serial command dispatch
  ├─ spawn imu_task            // Core 1 – 100 Hz sensor fusion
  ├─ spawn motor_task          // Core 1 – motor command queue
  │
  └─ state_machine::run()      // Core 0 – never returns
```

---

## Task Model

Each Embassy task is a single `async fn` that loops forever. Tasks are pinned to a core at spawn time using `embassy_executor::Spawner` (Core 0) or a secondary `embassy_executor::Executor` (Core 1).

| Task | Core | Rate / Trigger | Watchdog role |
|---|---|---|---|
| `heartbeat` | 0 | 1 Hz timer | Feeds MWDT every second |
| `wifi` | 0 | Event-driven | — |
| `can_bus` | 0 | 1 ms poll | — |
| `ui` | 0 | ~33 ms (30 fps) | — |
| `telemetry` | 0 | 1 Hz timer | — |
| `cli` | 0 | UART ISR / channel | — |
| `imu` | 1 | 10 ms (100 Hz) | — |
| `motor` | 1 | Channel receive + 50 ms safety tick | — |

### Safety timeout (motor task)

If the motor task receives no new command for 5 seconds it sends a `Brake` command automatically, ensuring the rover stops if the state machine or WiFi link hangs.

---

## Inter-Task Communication

All channels are **static** (`embassy_sync::channel::Channel`) backed by a `CriticalSectionRawMutex`. Producers and consumers never share mutable state directly.

```
                    ┌──────────────────────┐
                    │   state_machine      │
                    └──┬──────┬──────┬────┘
          MOTOR_CHANNEL│      │IP_CH │UI_DRAW_CHANNEL
                       ▼      ▼      ▼
                  motor_task  (self) ui_task
                              ▲
                    wifi_task─┘

     IMU_CHANNEL: imu_task ──────────────► state_machine
 COLLISION_CHANNEL: can_bus_task ─────────► state_machine
   CAN_RX_CHANNEL: can_bus_task ──────────► (user code)
   CAN_TX_CHANNEL: (user code) ───────────► can_bus_task
 CLI_COMMAND_CHANNEL: push_byte ISR ──────► cli_task
```

| Channel | Type | Depth | Sender → Receiver |
|---|---|---|---|
| `MOTOR_CHANNEL` | `MotorCommand` | 8 | state_machine → motor_task |
| `IMU_CHANNEL` | `ImuReading` | 2 | imu_task → state_machine |
| `UI_DRAW_CHANNEL` | `UiDrawCommand` | 8 | state_machine → ui_task |
| `IP_CHANNEL` | `String<16>` | 1 | wifi_task → state_machine |
| `COLLISION_CHANNEL` | `CanFrame` | 4 | can_bus_task → state_machine |
| `CAN_RX_CHANNEL` | `CanFrame` | 16 | can_bus_task → user code |
| `CAN_TX_CHANNEL` | `CanFrame` | 8 | user code → can_bus_task |
| `CLI_COMMAND_CHANNEL` | `String<64>` | 4 | UART ISR → cli_task |

---

## Agentic State Machine

The state machine runs the top-level sense-think-act loop. It is event-driven and transitions between the following states:

```
        ┌──────────────────────────────────────────────────────┐
        │                (collision alert – any state)         │
        │                           ▼                          │
  ┌─────┴──┐   WiFi ready   ┌──────────────┐                   │
  │  Idle  ├───────────────►│  Listening   │                   │
  └────────┘                └──────┬───────┘                   │
       ▲                           │ audio captured             │
       │                           ▼                            │
       │                   ┌──────────────┐                     │
       │                   │  Processing  │ (LLM API over WiFi) │
       │                   └──────┬───────┘                     │
       │                          │ motion command               │
       │                          ▼                              │
       │                   ┌──────────────┐                     │
       │                   │   Moving     │                     │
       │                   └──────┬───────┘                     │
       │                          │ motion done                  │
       │                          ▼                              │
       │                   ┌──────────────┐   ┌───────────────┐ │
       └───────────────────│  Reporting   │   │ EmergencyStop │◄┘
                           └──────────────┘   └───────────────┘
```

### State descriptions

| State | Description |
|---|---|
| `Idle` | Waits for a WiFi IP from `IP_CHANNEL` or a 2-second idle tick |
| `Listening` | Simulates 1 s of 16 kHz/16-bit mono audio capture; updates a progress bar on the display |
| `Processing` | Sends the audio payload to an LLM API via TCP; parses the JSON response to extract a motion command |
| `Moving` | Dispatches a `MotorCommand` through `MOTOR_CHANNEL`; waits 2 s for the motion to complete |
| `Reporting` | Drains the latest `ImuReading` from `IMU_CHANNEL`; logs attitude; returns to `Idle` |
| `EmergencyStop` | Brakes immediately; flashes the display red; waits for an external clear signal |

### Motion command grammar

The LLM response is parsed by a tiny no-std text parser into one of:

```
"forward  <speed>"   →  MotorCommand::Forward(speed)   speed ∈ [0.0, 1.0]
"backward <speed>"   →  MotorCommand::Backward(speed)
"rotate   <angle>"   →  MotorCommand::Rotate(angle)    angle in degrees
"stop"  / "brake"    →  MotorCommand::Brake
```

---

## Hardware Abstraction Layer (HAL)

The HAL defines **Rust traits** that decouple the state machine and tasks from any specific hardware implementation.

### `RoverChassis` (`hal/chassis.rs`)

```
RoverChassis
 ├── move_forward(speed: f32)    // normalised 0.0–1.0
 ├── move_backward(speed: f32)
 ├── rotate(angle_degrees: f32)  // open-loop turn
 ├── brake()                     // active motor braking
 └── stop()                      // free-wheel coast
```

Concrete implementation: `L298nChassis` (wraps the `L298N` driver, translates normalised speed to 8-bit PWM duty cycle).

### `SpatialSensor` (`hal/sensor.rs`)

```
SpatialSensor
 ├── read_acceleration() → Result<Vector3D>   // m/s²
 ├── read_angular_velocity() → Result<Vector3D> // rad/s
 └── read_distance() → Result<u16>            // mm
```

Concrete implementation: `CombinedSpatialSensor` (aggregates `Mpu6500` + `Vl53l0x`).

`Vector3D` is a lightweight 3-axis value type with `add`, `sub`, `normalize`, and `magnitude` operations — no floating-point allocations.

### `AudioInput` / `AudioOutput` (`hal/audio.rs`)

Async traits for I²S audio capture (INMP441) and playback (MAX98357A). Driver stubs are provided; full DMA integration is left for future work.

---

## Device Drivers

| Driver | File | Interface | Key details |
|---|---|---|---|
| MPU-6500 | `drivers/mpu6500.rs` | I²C (0x68 / 0x69) | ±2 g / ±250 °/s, 16-bit; reads ACCEL+GYRO in one burst (14 bytes) |
| VL53L0X | `drivers/vl53l0x.rs` | I²C (0x29) | Single-shot ranging; polls measurement-complete flag; 200 ms max wait |
| L298N | `drivers/l298n.rs` | GPIO + LEDC PWM | `set_left/set_right(duty, forward)` per motor; `brake()` / `coast()` modes |
| ST7789 | `drivers/st7789.rs` | SPI | 240 × 240, RGB565; hardware reset; `set_address_window` + `flush(buf)` |

---

## Memory Layout

```
┌─────────────────────────────────────────────────────┐
│  Internal SRAM  (512 KB)                            │
│  ┌──────────────────────────────────────────────┐   │
│  │  esp_alloc SRAM heap                         │   │
│  │  • execution stacks                          │   │
│  │  • Embassy task arenas (20 480 B)            │   │
│  │  • DMA descriptors                           │   │
│  │  • heapless channel buffers                  │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│  Octal PSRAM  (8 MB)                                │
│  ┌──────────────────────────────────────────────┐   │
│  │  esp_alloc PSRAM heap                        │   │
│  │  • ST7789 framebuffer (115 200 B, RGB565)    │   │
│  │  • Audio capture/playback ring buffers       │   │
│  │  • Large heap allocations from alloc crate   │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

The allocator is initialised in `memory.rs` at boot via `init_heap()` (SRAM) and the `init_psram!` macro (PSRAM). The `telemetry` task reports `heap_used()` / `heap_free()` every second.

---

## CAN Bus Message Routing

The TWAI peripheral runs at 500 kbps. Incoming frames are dispatched by ID range:

| ID range | Priority | Channel |
|---|---|---|
| `0x000–0x0FF` | Highest — collision alerts | `COLLISION_CHANNEL` (depth 4) |
| `0x100–0x1FF` | Sensor data (IMU, ToF) | `CAN_RX_CHANNEL` (depth 16) |
| `0x200–0x2FF` | Telemetry / status | `CAN_RX_CHANNEL` |
| `0x300–0x7FF` | Configuration | `CAN_RX_CHANNEL` |

Frames queued on `CAN_TX_CHANNEL` are forwarded to the TWAI TX FIFO by `can_bus_task` with a 1 ms polling period.

---

## Display Pipeline

```
state_machine / tasks
        │  UiDrawCommand (enum)
        ▼
   UI_DRAW_CHANNEL  (depth 8)
        │
        ▼
   ui_task (Core 0, ~30 fps)
        │
        ├── Clear(color)           → fill framebuffer
        ├── StatusText(text)       → embedded-graphics text at top
        ├── ShowState(name)        → yellow state label
        └── ProgressBar { %, lbl } → green filled rectangle + label
        │
        ▼
   framebuffer Vec<u8>  (PSRAM, 115 200 B, RGB565)
        │
        ▼
   ST7789::flush()  (SPI DMA transfer)
        │
        ▼
   240 × 240 TFT panel
```

The framebuffer is allocated from PSRAM to keep the internal SRAM heap available for time-critical DMA and stack allocations.
