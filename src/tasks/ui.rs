//! # UI Task
//!
//! Manages a virtual framebuffer stored in PSRAM and synchronises it to the
//! ST7789 SPI display at the target frame rate.
//!
//! ## Pipeline
//! ```text
//!  UI_DRAW_CHANNEL  ──►  compose()  ──►  PSRAM framebuffer  ──►  SPI DMA  ──►  ST7789
//! ```
//!
//! * [`UiDrawCommand`] messages are produced by the state machine and queued on
//!   [`UI_DRAW_CHANNEL`].
//! * The `compose` step renders each command onto the framebuffer using
//!   `embedded-graphics`.
//! * The SPI sync step DMA-transfers the framebuffer to the display.

extern crate alloc;

use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;

use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};

// ---------------------------------------------------------------------------
// Display constants
// ---------------------------------------------------------------------------

/// Horizontal resolution of the SSD1306 display in pixels (Wokwi).
pub const DISPLAY_WIDTH: usize = 128;
/// Vertical resolution of the SSD1306 display in pixels (Wokwi).
pub const DISPLAY_HEIGHT: usize = 64;
/// Bytes per pixel – RGB565 → 2 bytes.
pub const BYTES_PER_PIXEL: usize = 2;
/// Total framebuffer size in bytes.
pub const FRAMEBUFFER_BYTES: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT * BYTES_PER_PIXEL;
/// Target frame rate in Hz.
pub const TARGET_FPS: u64 = 30;
/// Frame period in milliseconds.
pub const FRAME_PERIOD_MS: u64 = 1_000 / TARGET_FPS;

// ---------------------------------------------------------------------------
// Draw command type
// ---------------------------------------------------------------------------

/// A high-level draw command sent from the state machine to the UI task.
#[derive(Debug, Clone)]
pub enum UiDrawCommand {
    /// Clear the display to the given RGB565 colour.
    Clear(Rgb565),
    /// Print a status string at the top of the screen.
    StatusText(heapless::String<64>),
    /// Show the current state name.
    ShowState(heapless::String<32>),
    /// Draw a simple horizontal progress bar (0–100 %).
    ProgressBar { percent: u8, label: heapless::String<16> },
}

/// Depth of the draw-command queue.
pub const UI_CHANNEL_DEPTH: usize = 8;

/// Channel through which the state machine sends [`UiDrawCommand`] to the UI
/// task.
pub static UI_DRAW_CHANNEL: Channel<CriticalSectionRawMutex, UiDrawCommand, UI_CHANNEL_DEPTH> =
    Channel::new();

// Note: Framebuffer implementation removed for Wokwi - we draw directly to display

// ---------------------------------------------------------------------------
// Compose helper
// ---------------------------------------------------------------------------

/// Apply a single [`UiDrawCommand`] to the display.
fn compose<D>(display: &mut D, cmd: &UiDrawCommand)
where
    D: DrawTarget<Color = BinaryColor, Error = core::convert::Infallible>,
{
    match cmd {
        UiDrawCommand::Clear(color) => {
            let mono = if *color == Rgb565::BLACK {
                BinaryColor::Off
            } else {
                BinaryColor::On
            };
            display.clear(mono).ok();
        }

        UiDrawCommand::StatusText(text) => {
            Rectangle::new(Point::new(0, 0), Size::new(DISPLAY_WIDTH as u32, 12))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
                .draw(display)
                .ok();
            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            Text::new(text.as_str(), Point::new(0, 10), style)
                .draw(display)
                .ok();
        }

        UiDrawCommand::ShowState(state) => {
            // Clear a region for the state text
            Rectangle::new(Point::new(0, 14), Size::new(DISPLAY_WIDTH as u32, 12))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
                .draw(display)
                .ok();

            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            Text::new(state.as_str(), Point::new(0, 24), style)
                .draw(display)
                .ok();
        }

        UiDrawCommand::ProgressBar { percent, label } => {
            // Clear progress bar area
            Rectangle::new(Point::new(0, 34), Size::new(DISPLAY_WIDTH as u32, 30))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
                .draw(display)
                .ok();

            let bar_width = ((DISPLAY_WIDTH as u32 - 2) * (*percent as u32)) / 100;
            Rectangle::new(Point::new(1, 52), Size::new(bar_width, 10))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(display)
                .ok();
            let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
            Text::new(label.as_str(), Point::new(0, 44), style)
                .draw(display)
                .ok();
        }
    }
}

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task that manages the display and processes draw commands.
///
/// Spawn from `main`:
/// ```rust,no_run
/// spawner.spawn(ui_task(i2c)).unwrap();
/// ```
#[task]
pub async fn ui_task(i2c: esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>) {
    esp_println::println!("[ui] Task started – {}x{} @ {} fps", DISPLAY_WIDTH, DISPLAY_HEIGHT, TARGET_FPS);

    // Initialize SSD1306 driver
    esp_println::println!("[ui] Initializing SSD1306 driver...");
    let mut display = crate::drivers::ssd1306::Ssd1306Driver::new(i2c);
    esp_println::println!("[ui] Display driver initialized!");

    // Paint the initial screen
    esp_println::println!("[ui] Clearing display to black...");
    let clear_result = display.clear(BinaryColor::Off);
    let _ = display.flush();
    esp_println::println!("[ui] Clear result: {:?}", clear_result);

    esp_println::println!("[ui] Creating text style...");
    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);

    esp_println::println!("[ui] Drawing initial text...");
    let text = Text::new("EspOS SSD1306 OK", Point::new(0, 10), style);
    let draw_result = text.draw(&mut display);
    let _ = display.flush();
    esp_println::println!("[ui] Draw result: {:?}", draw_result);
    esp_println::println!("[ui] Initial screen drawn, entering main loop");

    loop {
        // Wait for a draw command with timeout
        match embassy_time::with_timeout(
            embassy_time::Duration::from_millis(FRAME_PERIOD_MS),
            UI_DRAW_CHANNEL.receive()
        ).await {
            Ok(cmd) => {
                log::info!("[ui] Received command: {:?}", cmd);
                // Process this command
                compose(&mut display, &cmd);
                let _ = display.flush();

                // Drain any additional pending commands (batch processing)
                while let Ok(cmd) = UI_DRAW_CHANNEL.try_receive() {
                    log::debug!("[ui] Batch command: {:?}", cmd);
                    compose(&mut display, &cmd);
                }
                let _ = display.flush();
            }
            Err(_timeout) => {
                // No commands received, just tick at frame rate
            }
        }

        Timer::after_millis(FRAME_PERIOD_MS).await;
    }
}
