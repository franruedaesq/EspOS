//! # Heartbeat Task
//!
//! Blinks the on-board LED at 1 Hz to provide a visual "system alive"
//! indicator.  The task runs on Core 0 and uses [`embassy_time::Timer`] for
//! non-blocking delays so the executor can schedule other work between blinks.

use embassy_executor::task;
use embassy_time::Timer;
use esp_hal::gpio::Output;

/// Embassy task that toggles `led` every 500 ms (1 Hz blink).
///
/// Spawn once from `main`:
/// ```rust,no_run
/// spawner.spawn(heartbeat_task(led)).unwrap();
/// ```
#[task]
pub async fn heartbeat_task(mut led: Output<'static>) {
    log::info!("[heartbeat] started");

    loop {
        led.set_high();
        Timer::after_millis(500).await;

        led.set_low();
        Timer::after_millis(500).await;
    }
}
