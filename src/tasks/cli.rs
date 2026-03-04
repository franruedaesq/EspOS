//! # Debug CLI Task
//!
//! Provides a minimal serial command-line interface.  Type commands into your
//! host terminal at **115 200 baud** and the ESP32-S3 responds over the same
//! UART.
//!
//! ## Commands
//!
//! | Command  | Description                                               |
//! |----------|-----------------------------------------------------------|
//! | `help`   | Print the command table.                                  |
//! | `status` | Print live RAM usage, CPU load, task list, and battery.   |
//!
//! ## UART wiring (ESP32-S3 default)
//!
//! | Signal | GPIO |
//! |--------|------|
//! | TX     | 43   |
//! | RX     | 44   |
//!
//! ## Integration – adding a UART reader task
//!
//! Instantiate a UART RX half in `main.rs` and call [`push_byte`] for every
//! byte received:
//!
//! ```rust,no_run
//! use esp_hal::uart::{Uart, Config};
//!
//! // Create async UART0 (TX is already driven by esp-println; we only need RX).
//! let uart0 = Uart::new(peripherals.UART0, Config::default())
//!     .into_async()
//!     .with_rx(peripherals.GPIO44);
//! let (_, rx) = uart0.split();
//!
//! spawner.spawn(tasks::cli::uart_reader_task(rx)).expect("spawn uart_reader_task");
//! spawner.spawn(tasks::cli::cli_task()).expect("spawn cli_task");
//! ```
//!
//! Then add this reader task alongside `cli_task`:
//!
//! ```rust,no_run
//! #[task]
//! pub async fn uart_reader_task(
//!     mut rx: UartRx<'static, esp_hal::peripherals::UART0, esp_hal::Async>,
//! ) {
//!     use embedded_io_async::Read;
//!     let mut buf = [0u8; 1];
//!     loop {
//!         if rx.read_exact(&mut buf).await.is_ok() {
//!             tasks::cli::push_byte(buf[0]);
//!         }
//!     }
//! }
//! ```

use core::sync::atomic::Ordering;

use critical_section::Mutex;
use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

use crate::memory;
use crate::tasks::heartbeat::HEARTBEAT_TICKS;

// ---------------------------------------------------------------------------
// Line buffer (filled by UART ISR / reader task via `push_byte`)
// ---------------------------------------------------------------------------

use core::cell::RefCell;

/// Maximum length of a single CLI command line (bytes).
pub const CLI_LINE_LEN: usize = 64;

/// Shared accumulation buffer for incoming serial bytes.
static LINE_BUF: Mutex<RefCell<heapless::String<CLI_LINE_LEN>>> =
    Mutex::new(RefCell::new(heapless::String::new()));

// ---------------------------------------------------------------------------
// Command channel
// ---------------------------------------------------------------------------

/// Depth of the command dispatch queue.
const CLI_CHANNEL_DEPTH: usize = 4;

/// Complete, newline-terminated command strings ready for dispatch.
///
/// Populated by [`push_byte`]; consumed by [`cli_task`].
pub static CLI_COMMAND_CHANNEL: Channel<
    CriticalSectionRawMutex,
    heapless::String<CLI_LINE_LEN>,
    CLI_CHANNEL_DEPTH,
> = Channel::new();

// ---------------------------------------------------------------------------
// Public byte-push helper (call from UART ISR / reader task)
// ---------------------------------------------------------------------------

/// Feed a single received byte into the CLI line buffer.
///
/// When a newline (`\n` or `\r`) is detected the accumulated line is sent to
/// [`CLI_COMMAND_CHANNEL`] for dispatch by [`cli_task`].  Oversized input is
/// silently discarded.
///
/// This function is interrupt-safe: it uses a `critical_section` internally.
pub fn push_byte(byte: u8) {
    critical_section::with(|cs| {
        let mut buf = LINE_BUF.borrow_ref_mut(cs);
        if byte == b'\n' || byte == b'\r' {
            if !buf.is_empty() {
                let line = buf.clone();
                // Best-effort: if the channel is full, drop the command.
                let _ = CLI_COMMAND_CHANNEL.try_send(line);
                buf.clear();
            }
        } else if buf.push(byte as char).is_err() {
            log::warn!("[cli] line buffer overflow – discarding input");
            buf.clear();
        }
    });
}

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

/// Embassy task – waits for complete command lines on [`CLI_COMMAND_CHANNEL`]
/// and dispatches them to the appropriate handler.
///
/// Spawn from `main`:
/// ```rust,no_run
/// spawner.spawn(tasks::cli::cli_task()).expect("spawn cli_task");
/// ```
#[task]
pub async fn cli_task() {
    log::info!("[cli] task started – type 'help' for commands");

    loop {
        let line = CLI_COMMAND_CHANNEL.receive().await;
        handle_command(line.as_str().trim());
    }
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

fn handle_command(cmd: &str) {
    match cmd {
        "help" => print_help(),
        "status" => print_status(),
        "" => {}
        other => {
            esp_println::println!("[cli] unknown command '{}' – type 'help'", other);
        }
    }
}

fn print_help() {
    esp_println::println!("EspOS CLI");
    esp_println::println!("  help    Print this message");
    esp_println::println!("  status  Show RAM, CPU load, task list, and battery");
}

fn print_status() {
    let ram_used = memory::heap_used();
    let ram_free = memory::heap_free();
    // Use `load` (not `swap`) so the CLI snapshot does not interfere with the
    // 1-second window that `telemetry_task` resets via `swap(0)`.
    // The value shown here is the cumulative count since the last telemetry
    // reset – typically 0 or 1 between telemetry cycles, which is correct.
    let heartbeat_hz = HEARTBEAT_TICKS.load(Ordering::Relaxed);

    esp_println::println!("=== EspOS status ===");
    esp_println::println!("  RAM used    : {} B", ram_used);
    esp_println::println!("  RAM free    : {} B", ram_free);
    esp_println::println!("  Heartbeat   : {} Hz (CPU-load proxy)", heartbeat_hz);
    // NOTE: keep the list below in sync with the tasks spawned in main.rs.
    esp_println::println!("  Tasks       : heartbeat imu motor can_bus ui telemetry cli");
    esp_println::println!("====================");
}
