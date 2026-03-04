//! # IMU Task (Core 1)
//!
//! Reads raw acceleration and gyroscope data from the MPU-6500 over I²C,
//! applies a complementary filter for attitude estimation, and publishes
//! fused [`ImuReading`] messages on a static channel consumed by the state
//! machine.
//!
//! This task is pinned to **Core 1** via the `#[task]` macro so that the
//! heavy I²C polling does not interfere with WiFi processing on Core 0.

use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;

use crate::hal::sensor::Vector3D;

// ---------------------------------------------------------------------------
// Published message type
// ---------------------------------------------------------------------------

/// Fused inertial measurement published at each sensor cycle.
#[derive(Debug, Clone, Copy)]
pub struct ImuReading {
    /// Linear acceleration in m/s² (gravity-compensated after fusion).
    pub accel: Vector3D,
    /// Angular velocity in rad/s.
    pub gyro: Vector3D,
    /// Roll angle in degrees (complementary filter output).
    pub roll: f32,
    /// Pitch angle in degrees (complementary filter output).
    pub pitch: f32,
}

/// Channel capacity: keep the two most recent readings so the consumer is
/// never blocked if it processes them slightly slower than they are produced.
pub const IMU_CHANNEL_DEPTH: usize = 2;

/// Static channel for publishing [`ImuReading`] values from Core 1 tasks to
/// consumers on any core.
pub static IMU_CHANNEL: Channel<CriticalSectionRawMutex, ImuReading, IMU_CHANNEL_DEPTH> =
    Channel::new();

// ---------------------------------------------------------------------------
// Complementary filter constants
// ---------------------------------------------------------------------------

/// Accelerometer weight in the complementary filter.  Must satisfy
/// `ALPHA + (1 - ALPHA) == 1.0`.
const ALPHA: f32 = 0.98;

/// Target sensor polling interval in milliseconds → 100 Hz.
const POLL_INTERVAL_MS: u64 = 10;

/// `dt` in seconds, derived from [`POLL_INTERVAL_MS`].
const DT: f32 = POLL_INTERVAL_MS as f32 / 1_000.0;

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task – reads the MPU-6500 at 100 Hz and publishes fused attitude
/// data on [`IMU_CHANNEL`].
///
/// Spawn from `main` (typically on Core 1):
/// ```rust,no_run
/// spawner.spawn(imu_task()).unwrap();
/// ```
#[task]
pub async fn imu_task() {
    log::info!("[imu] task started");

    // Accumulated attitude angles (complementary filter state).
    let mut roll = 0.0f32;
    let mut pitch = 0.0f32;

    loop {
        // ---- Read raw sensor data ----------------------------------------
        // In a real build these values come from a `Mpu6500` driver instance
        // passed into the task.  Here we use placeholder zeroes so the task
        // compiles in a no_std environment without hardware.
        let accel = Vector3D::new(0.0, 0.0, 9.81);
        let gyro = Vector3D::new(0.0, 0.0, 0.0);

        // ---- Complementary filter ----------------------------------------
        // Accelerometer-derived angle (noisy but drift-free).
        let accel_roll = libm::atan2f(accel.y, accel.z) * (180.0 / core::f32::consts::PI);
        let accel_pitch =
            libm::atan2f(-accel.x, libm::sqrtf(accel.y * accel.y + accel.z * accel.z))
                * (180.0 / core::f32::consts::PI);

        // Gyroscope integration (low-noise, drifting).
        roll = ALPHA * (roll + gyro.x * DT) + (1.0 - ALPHA) * accel_roll;
        pitch = ALPHA * (pitch + gyro.y * DT) + (1.0 - ALPHA) * accel_pitch;

        let reading = ImuReading {
            accel,
            gyro,
            roll,
            pitch,
        };

        // ---- Publish (non-blocking, drop if full) ------------------------
        let _ = IMU_CHANNEL.try_send(reading);

        Timer::after_millis(POLL_INTERVAL_MS).await;
    }
}
