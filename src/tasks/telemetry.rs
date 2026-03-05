//! # Telemetry Task
//!
//! Samples system health metrics every second and publishes them as a compact
//! JSON string.  The metrics are:
//!
//! | Field            | Source                                              |
//! |------------------|-----------------------------------------------------|
//! | `ram_used`       | Global heap allocator (`ALLOCATOR.used()`)          |
//! | `ram_free`       | Global heap allocator (`ALLOCATOR.free()`)          |
//! | `cpu_percent`    | Estimated CPU load (100% - idle time)               |
//! | `heartbeat_hz`   | [`HEARTBEAT_TICKS`] counter reset every 1 s         |
//! | `battery_mv`     | ADC pin (placeholder until hardware is wired)       |
//!
//! ## MQTT integration
//!
//! Once WiFi is active you can publish the JSON to an MQTT broker:
//!
//! 1. Add to `Cargo.toml`:
//!    ```toml
//!    rust-mqtt = { version = "0.3", default-features = false, features = ["no-std"] }
//!    ```
//! 2. Open a TCP socket via `embassy-net`:
//!    ```rust,no_run
//!    let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
//!    socket.connect((broker_ip, 1883)).await.unwrap();
//!    ```
//! 3. Publish the JSON payload:
//!    ```rust,no_run
//!    let mut client = MqttClient::new(socket, …);
//!    client.send_message("espos/telemetry", json.as_bytes(),
//!                         QualityOfService::QoS0, false).await.unwrap();
//!    ```
//!
//! Your React front-end can subscribe to `espos/telemetry` and draw live graphs
//! from the streamed JSON objects.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration, Instant, Timer};

use crate::memory;
use crate::tasks::heartbeat::HEARTBEAT_TICKS;

// CPU idle time tracking (microseconds spent sleeping)
static IDLE_TIME_US: AtomicU32 = AtomicU32::new(0);

/// Last measured CPU usage percentage (0–100). Updated every 1 s.
/// Read by the UI task to display live stats on screen.
pub static LAST_CPU_PERCENT: AtomicU32 = AtomicU32::new(0);

/// Record idle/sleep time for CPU usage estimation.
/// Call this before entering long sleep periods.
pub fn record_idle(duration: Duration) {
    IDLE_TIME_US.fetch_add(duration.as_micros() as u32, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task – samples health metrics every 1 s and emits a JSON log line.
///
/// Spawn from `main`:
/// ```rust,no_run
/// spawner.spawn(telemetry_task()).unwrap();
/// ```
#[task]
pub async fn telemetry_task() {
    log::info!("[telemetry] task started");

    loop {
        let start = Instant::now();
        Timer::after_millis(1_000).await;
        let elapsed_us = start.elapsed().as_micros() as u32;

        // ---- RAM usage --------------------------------------------------
        let ram_used = memory::heap_used();
        let ram_free = memory::heap_free();

        // ---- CPU load ---------------------------------------------------
        // Estimate CPU usage: 100% - (idle_time / total_time * 100)
        let idle_us = IDLE_TIME_US.swap(0, Ordering::Relaxed);
        let cpu_percent = if elapsed_us > 0 {
            100u32.saturating_sub((idle_us * 100) / elapsed_us)
        } else {
            0
        };
        LAST_CPU_PERCENT.store(cpu_percent, Ordering::Relaxed);

        // ---- CPU load (heartbeat ticks per second) ----------------------
        // Atomically swap the counter with 0 so each 1-second window is
        // measured independently.  A value of 1 means the heartbeat looped
        // once in the last second (the normal, healthy rate).
        let heartbeat_hz = HEARTBEAT_TICKS.swap(0, Ordering::Relaxed);

        // ---- Battery voltage (ADC – placeholder) ------------------------
        // To connect a real LiPo cell:
        //   1. Wire the battery positive terminal through a 100 kΩ / 100 kΩ
        //      voltage divider to GPIO1.
        //   2. Replace the constant below with:
        //
        //   use esp_hal::analog::adc::{Adc, AdcConfig, Attenuation};
        //   let mut adc_config = AdcConfig::new();
        //   let mut bat_pin = adc_config
        //       .enable_pin(gpio1, Attenuation::Attenuation11dB);
        //   let mut adc = Adc::new(peripherals.ADC1, adc_config);
        //   let raw: u16 = nb::block!(adc.read_oneshot(&mut bat_pin))
        //       .unwrap_or(0);
        //   // ADC reference is 3.3 V, 12-bit; divider ratio = 2.
        //   let battery_mv = (raw as u32 * 3_300 * 2 / 4095) as u16;
        let battery_mv: u16 = 3_700; // placeholder: 3.7 V LiPo at rest

        // ---- Build JSON (heapless – zero heap allocation) ---------------
        let mut json: heapless::String<256> = heapless::String::new();
        let _ = core::fmt::write(
            &mut json,
            format_args!(
                concat!(
                    r#"{{"ram_used":{ram},"ram_free":{free},"#,
                    r#""cpu_percent":{cpu},"heartbeat_hz":{hz},"battery_mv":{batt}}}"#,
                ),
                ram  = ram_used,
                free = ram_free,
                cpu  = cpu_percent,
                hz   = heartbeat_hz,
                batt = battery_mv,
            ),
        );

        log::info!("[telemetry] {}", json.as_str());

        // ---- MQTT publish -----------------------------------------------
        // Uncomment and complete the MQTT steps described in the module
        // doc-comment above to stream this JSON to your broker.
    }
}
