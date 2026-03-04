//! # CAN Bus / TWAI Task
//!
//! Initialises the ESP32-S3 TWAI peripheral with an SN65HVD230 transceiver
//! and implements a priority-based message queue.  High-priority collision
//! alerts pre-empt lower-priority telemetry so the state machine reacts to
//! safety-critical events immediately.
//!
//! ## Message Priority Scheme
//! | Priority | CAN ID range | Description                        |
//! |----------|--------------|------------------------------------|
//! | 0 (high) | 0x000–0x0FF  | Collision / emergency stop         |
//! | 1        | 0x100–0x1FF  | Sensor data (IMU, ToF)             |
//! | 2        | 0x200–0x2FF  | Telemetry / status                 |
//! | 3 (low)  | 0x300–0x7FF  | Configuration / non-critical data  |

use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;

// ---------------------------------------------------------------------------
// CAN frame type
// ---------------------------------------------------------------------------

/// A received or transmitted CAN 2.0A frame (11-bit standard ID, ≤8 data
/// bytes).
#[derive(Debug, Clone, Copy)]
pub struct CanFrame {
    /// Standard 11-bit identifier.
    pub id: u16,
    /// Number of valid bytes in [`data`] (`0..=8`).
    pub dlc: u8,
    /// Frame payload.
    pub data: [u8; 8],
}

impl CanFrame {
    /// Returns the priority tier `0–3` derived from the frame identifier.
    pub fn priority(&self) -> u8 {
        match self.id {
            0x000..=0x0FF => 0,
            0x100..=0x1FF => 1,
            0x200..=0x2FF => 2,
            _ => 3,
        }
    }

    /// Returns `true` if this is a collision / emergency-stop frame.
    pub fn is_collision(&self) -> bool {
        self.priority() == 0
    }
}

// ---------------------------------------------------------------------------
// Inter-task channels
// ---------------------------------------------------------------------------

/// Depth of the inbound CAN frame queue.
pub const CAN_RX_DEPTH: usize = 16;

/// Inbound frames from the TWAI peripheral → state machine.
pub static CAN_RX_CHANNEL: Channel<CriticalSectionRawMutex, CanFrame, CAN_RX_DEPTH> =
    Channel::new();

/// Outbound frames queued by other tasks → TWAI peripheral.
pub static CAN_TX_CHANNEL: Channel<CriticalSectionRawMutex, CanFrame, 8> = Channel::new();

/// High-priority collision alert channel (depth 4 to absorb burst).
pub static COLLISION_CHANNEL: Channel<CriticalSectionRawMutex, CanFrame, 4> = Channel::new();

// ---------------------------------------------------------------------------
// TWAI configuration helpers
// ---------------------------------------------------------------------------

/// Bit-timing for 500 kbps with an 80 MHz APB clock (SN65HVD230 compatible).
///
/// Timing segments: SYNC=1, PROP+SEG1=15, SEG2=4 TQ → BRP=10.
/// Sample point ≈ 80 % – within the CiA 301 recommended range.
pub struct TwaiBitTiming {
    pub brp: u16,
    pub tseg1: u8,
    pub tseg2: u8,
    pub sjw: u8,
}

impl TwaiBitTiming {
    /// 500 kbps preset.
    pub const fn kbps_500() -> Self {
        Self {
            brp: 10,
            tseg1: 15,
            tseg2: 4,
            sjw: 3,
        }
    }

    /// 250 kbps preset.
    pub const fn kbps_250() -> Self {
        Self {
            brp: 20,
            tseg1: 15,
            tseg2: 4,
            sjw: 3,
        }
    }
}

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task that drives the TWAI peripheral.
///
/// Responsibilities:
/// * Polls the hardware RX FIFO for incoming frames.
/// * Routes collision frames to [`COLLISION_CHANNEL`] (highest priority).
/// * Routes all other frames to [`CAN_RX_CHANNEL`].
/// * Drains [`CAN_TX_CHANNEL`] and transmits frames onto the bus.
///
/// Spawn from `main`:
/// ```rust,no_run
/// spawner.spawn(can_bus_task()).unwrap();
/// ```
#[task]
pub async fn can_bus_task() {
    log::info!("[can_bus] task started – 500 kbps");

    // In a production build the TWAI peripheral handle is passed in here
    // and the bit-timing registers are written using `TwaiBitTiming::kbps_500()`.

    loop {
        // ---- Simulated RX poll ------------------------------------------
        // In a real build: `twai.receive_async().await` or poll the RX FIFO.
        // Here we just yield so other tasks can run.
        Timer::after_millis(1).await;

        // ---- Drain TX queue ---------------------------------------------
        while let Ok(frame) = CAN_TX_CHANNEL.try_receive() {
            log::debug!(
                "[can_bus] TX id=0x{:03X} dlc={} data={:?}",
                frame.id,
                frame.dlc,
                &frame.data[..frame.dlc as usize]
            );
            // In a real build: `twai.transmit_async(&frame).await.unwrap();`
        }
    }
}

// ---------------------------------------------------------------------------
// Unified data router
// ---------------------------------------------------------------------------

/// Route a received CAN frame to the appropriate channel based on priority.
///
/// Call this from the TWAI RX interrupt handler or from inside
/// [`can_bus_task`] after receiving a frame.  CAN and WiFi data are treated
/// identically by the state machine once they reach their respective channels.
pub fn route_frame(frame: CanFrame) {
    if frame.is_collision() {
        // Best-effort: drop if the collision channel is full (shouldn't happen
        // with depth 4, but we must not block in an ISR context).
        let _ = COLLISION_CHANNEL.try_send(frame);
        log::warn!("[can_bus] collision alert id=0x{:03X}", frame.id);
    } else {
        let _ = CAN_RX_CHANNEL.try_send(frame);
    }
}
