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

use alloc::vec::Vec;

use embassy_executor::task;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;

use embedded_graphics::{
    mono_font::{ascii::FONT_9X18_BOLD, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};

// ---------------------------------------------------------------------------
// Display constants
// ---------------------------------------------------------------------------

/// Horizontal resolution of the ST7789 display in pixels.
pub const DISPLAY_WIDTH: usize = 240;
/// Vertical resolution of the ST7789 display in pixels.
pub const DISPLAY_HEIGHT: usize = 240;
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

// ---------------------------------------------------------------------------
// In-memory framebuffer
// ---------------------------------------------------------------------------

/// An RGB565 framebuffer backed by a heap-allocated `Vec<u8>`.
///
/// The allocation is performed in PSRAM (because PSRAM is registered with the
/// global allocator before this task starts) so the large buffer does not
/// consume precious internal SRAM.
pub struct Framebuffer {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}

impl Framebuffer {
    /// Allocate a new zeroed framebuffer (all pixels black) in PSRAM.
    pub fn new_psram() -> Self {
        let pixels = alloc::vec![0u8; FRAMEBUFFER_BYTES];
        Self {
            pixels,
            width: DISPLAY_WIDTH,
            height: DISPLAY_HEIGHT,
        }
    }

    /// Return an immutable slice of the raw pixel bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Write a single RGB565 pixel at `(x, y)`.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: Rgb565) {
        if x < self.width && y < self.height {
            let idx = (y * self.width + x) * BYTES_PER_PIXEL;
            let raw: u16 = RawU16::from(color).into_inner();
            self.pixels[idx] = (raw >> 8) as u8;
            self.pixels[idx + 1] = (raw & 0xFF) as u8;
        }
    }
}

// Implement the embedded-graphics `DrawTarget` so we can use the full
// embedded-graphics drawing API directly on the framebuffer.
impl DrawTarget for Framebuffer {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x >= 0
                && coord.y >= 0
                && (coord.x as usize) < self.width
                && (coord.y as usize) < self.height
            {
                self.set_pixel(coord.x as usize, coord.y as usize, color);
            }
        }
        Ok(())
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(self.width as u32, self.height as u32)
    }
}

use embedded_graphics::pixelcolor::raw::RawU16;

// ---------------------------------------------------------------------------
// Compose helper
// ---------------------------------------------------------------------------

/// Apply a single [`UiDrawCommand`] to the framebuffer.
fn compose(fb: &mut Framebuffer, cmd: &UiDrawCommand) {
    match cmd {
        UiDrawCommand::Clear(color) => {
            fb.clear(*color).ok();
        }

        UiDrawCommand::StatusText(text) => {
            let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE);
            Text::new(text.as_str(), Point::new(4, 18), style)
                .draw(fb)
                .ok();
        }

        UiDrawCommand::ShowState(state) => {
            let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::YELLOW);
            Text::new(state.as_str(), Point::new(4, 40), style)
                .draw(fb)
                .ok();
        }

        UiDrawCommand::ProgressBar { percent, label } => {
            let bar_width = (DISPLAY_WIDTH as u32 * (*percent as u32)) / 100;
            Rectangle::new(Point::new(4, 200), Size::new(bar_width, 16))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
                .draw(fb)
                .ok();
            let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::WHITE);
            Text::new(label.as_str(), Point::new(4, 196), style)
                .draw(fb)
                .ok();
        }
    }
}

// ---------------------------------------------------------------------------
// Task implementation
// ---------------------------------------------------------------------------

/// Embassy task that manages the PSRAM framebuffer and drives the ST7789.
///
/// Spawn from `main`:
/// ```rust,no_run
/// spawner.spawn(ui_task()).unwrap();
/// ```
#[task]
pub async fn ui_task() {
    log::info!("[ui] task started – {}×{} @ {} fps", DISPLAY_WIDTH, DISPLAY_HEIGHT, TARGET_FPS);

    // Allocate the framebuffer in PSRAM.
    let mut fb = Framebuffer::new_psram();

    // Paint the initial boot screen.
    fb.clear(Rgb565::BLACK).ok();
    let style = MonoTextStyle::new(&FONT_9X18_BOLD, Rgb565::CYAN);
    Text::new("EspOS booting…", Point::new(4, 60), style)
        .draw(&mut fb)
        .ok();

    loop {
        // ---- Drain pending draw commands ---------------------------------
        while let Ok(cmd) = UI_DRAW_CHANNEL.try_receive() {
            compose(&mut fb, &cmd);
        }

        // ---- SPI DMA transfer to ST7789 ---------------------------------
        // In a real build: `display.flush(fb.as_bytes()).await.unwrap();`
        // The ST7789 driver in `crate::drivers::st7789` wraps the SPI
        // peripheral and exposes a `flush()` async method.

        Timer::after_millis(FRAME_PERIOD_MS).await;
    }
}
