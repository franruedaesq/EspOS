//! # Heartbeat Task
//!
//! Blinks the on-board LED at 1 Hz to provide a visual "system alive"
//! indicator.  The task runs on Core 0 and uses [`embassy_time::Timer`] for
//! non-blocking delays so the executor can schedule other work between blinks.
//!
//! The task also:
//! * Increments [`HEARTBEAT_TICKS`] every full cycle so the telemetry task can
//!   compute a "heartbeats per second" CPU-load proxy.
//! * Feeds the hardware watchdog timer (TIMG1 MWDT) every cycle.  If this
//!   task ever stalls the ESP32-S3 will be forcefully rebooted.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration, Timer};
use esp_hal::gpio::Output;
use esp_hal::timer::timg::Wdt;

// ---------------------------------------------------------------------------
// Shared counter
// ---------------------------------------------------------------------------

/// Number of heartbeat cycles completed since the last telemetry snapshot.
///
/// Incremented once per 1-second blink cycle.  The telemetry task calls
/// [`AtomicU32::swap`] to read-and-reset the counter each second, obtaining a
/// "heartbeats-per-second" figure that serves as a coarse CPU-load indicator.
pub static HEARTBEAT_TICKS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task that toggles `led` every 500 ms (1 Hz blink), feeds the
/// hardware watchdog, and increments [`HEARTBEAT_TICKS`].
///
/// Spawn once from `main`:
/// ```rust,no_run
/// let timg1 = TimerGroup::new(peripherals.TIMG1);
/// let mut wdt1 = timg1.wdt;
/// wdt1.set_timeout(MwdtStage::Stage0, 10_000_000u64.micros());
/// wdt1.enable();
/// spawner.spawn(heartbeat_task(led, wdt1)).unwrap();
/// ```
#[task]
pub async fn heartbeat_task(
    mut led: Output<'static>,
    mut wdt: Wdt<esp_hal::peripherals::TIMG1>,
) {
    log::info!("[heartbeat] started");

    loop {
        led.set_high();
        crate::tasks::telemetry::record_idle(Duration::from_millis(500));
        Timer::after_millis(500).await;

        led.set_low();
        crate::tasks::telemetry::record_idle(Duration::from_millis(500));
        Timer::after_millis(500).await;

        // Increment the loop counter for the telemetry task first, then feed
        // the watchdog.  Incrementing first ensures that a hang anywhere in
        // the full cycle (including after the feed) can be detected on the
        // next pass because the counter will have stalled.
        HEARTBEAT_TICKS.fetch_add(1, Ordering::Relaxed);

        // Feed the hardware watchdog so it does not expire during normal
        // operation.  If this line is never reached the MCU reboots.
        wdt.feed();
    }
}
