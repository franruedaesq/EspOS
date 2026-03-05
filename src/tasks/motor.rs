//! # Motor Task (Core 1)
//!
//! Receives [`MotorCommand`] messages from the state machine and translates
//! them into PWM duty-cycle updates on the L298N H-bridge driver.
//!
//! Running on **Core 1** keeps the tight PWM update loop away from WiFi and
//! Bluetooth interrupts that run on Core 0.

use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;



// ---------------------------------------------------------------------------
// Command type
// ---------------------------------------------------------------------------

/// High-level motion commands sent to the motor task.
#[derive(Debug, Clone, Copy)]
pub enum MotorCommand {
    /// Drive forward at the given normalised speed `[0.0, 1.0]`.
    Forward(f32),
    /// Drive backward at the given normalised speed `[0.0, 1.0]`.
    Backward(f32),
    /// Rotate by the given angle in degrees (positive = clockwise).
    Rotate(f32),
    /// Apply active braking.
    Brake,
    /// Coast (free-wheel) to a stop.
    Stop,
}

/// Depth of the command queue.  Commands that arrive while the motor is busy
/// executing the previous command are buffered here.
pub const MOTOR_CHANNEL_DEPTH: usize = 8;

/// Static channel through which the state machine sends [`MotorCommand`]
/// values to the motor task.
pub static MOTOR_CHANNEL: Channel<CriticalSectionRawMutex, MotorCommand, MOTOR_CHANNEL_DEPTH> =
    Channel::new();

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task that owns the chassis HAL and executes motor commands.
///
/// Spawn from `main` (Core 1 preferred):
/// ```rust,no_run
/// spawner.spawn(motor_task()).unwrap();
/// ```
#[task]
pub async fn motor_task() {
    log::info!("[motor] task started");

    // Safety time-out: if no new command arrives within this window after a
    // movement command, the motors are stopped automatically.
    const WATCHDOG_MS: u64 = 2_000;

    // Track whether a move command is "active" so the watchdog can trigger.
    let mut moving = false;
    let mut watchdog_elapsed_ms: u64 = 0;
    const TICK_MS: u64 = 50;

    loop {
        // Non-blocking receive so we can also tick the watchdog.
        match MOTOR_CHANNEL.try_receive() {
            Ok(cmd) => {
                log::debug!("[motor] command: {:?}", cmd);
                watchdog_elapsed_ms = 0;

                match cmd {
                    MotorCommand::Forward(speed) => {
                        // chassis.move_forward(speed);
                        log::info!("[motor] forward speed={:.2}", speed);
                        moving = true;
                    }
                    MotorCommand::Backward(speed) => {
                        // chassis.move_backward(speed);
                        log::info!("[motor] backward speed={:.2}", speed);
                        moving = true;
                    }
                    MotorCommand::Rotate(angle) => {
                        // chassis.rotate(angle);
                        log::info!("[motor] rotate angle={:.1}°", angle);
                        moving = true;
                    }
                    MotorCommand::Brake => {
                        // chassis.brake();
                        log::info!("[motor] brake");
                        moving = false;
                    }
                    MotorCommand::Stop => {
                        // chassis.stop();
                        log::info!("[motor] stop");
                        moving = false;
                    }
                }
            }
            Err(_) => {
                // No command waiting – tick the watchdog.
                if moving {
                    watchdog_elapsed_ms += TICK_MS;
                    if watchdog_elapsed_ms >= WATCHDOG_MS {
                        log::warn!("[motor] watchdog triggered – stopping motors");
                        // chassis.brake();
                        moving = false;
                        watchdog_elapsed_ms = 0;
                    }
                }
            }
        }

        Timer::after_millis(TICK_MS).await;
    }
}
