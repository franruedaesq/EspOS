//! # ST7789 SPI Display Driver
//!
//! Drives an ST7789 TFT display (240×240 RGB565) over SPI using the
//! `esp-hal` SPI master peripheral.
//!
//! ## Wiring
//! ```text
//!  ESP32-S3   ST7789
//!  ─────────  ──────
//!  SCK   ──►  SCL
//!  MOSI  ──►  SDA
//!  CS    ──►  CS
//!  DC    ──►  DC
//!  RST   ──►  RST
//!  3.3V  ──►  BL  (backlight always on)
//! ```
//!
//! ## Coordinate system
//! Origin (0, 0) is the top-left corner.  X increases to the right, Y
//! increases downward.

use esp_hal::gpio::Output;

// ---------------------------------------------------------------------------
// Display constants
// ---------------------------------------------------------------------------

/// Display width in pixels.
pub const WIDTH: u16 = 240;
/// Display height in pixels.
pub const HEIGHT: u16 = 240;

// ---------------------------------------------------------------------------
// ST7789 command codes
// ---------------------------------------------------------------------------

#[allow(dead_code)]
mod cmd {
    pub const NOP: u8 = 0x00;
    pub const SWRESET: u8 = 0x01;
    pub const SLPIN: u8 = 0x10;
    pub const SLPOUT: u8 = 0x11;
    pub const NORON: u8 = 0x13;
    pub const INVOFF: u8 = 0x20;
    pub const INVON: u8 = 0x21;
    pub const DISPON: u8 = 0x29;
    pub const CASET: u8 = 0x2A;
    pub const RASET: u8 = 0x2B;
    pub const RAMWR: u8 = 0x2C;
    pub const MADCTL: u8 = 0x36;
    pub const COLMOD: u8 = 0x3A;
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur when communicating with the ST7789.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum St7789Error {
    /// SPI transfer failed.
    SpiError,
    /// GPIO operation failed.
    GpioError,
}

// ---------------------------------------------------------------------------
// Driver struct
// ---------------------------------------------------------------------------

/// Driver for the ST7789 240×240 TFT display.
///
/// In a real build the SPI field holds an `esp_hal::spi::master::Spi` handle.
/// It is left as a `()` here so the driver type-checks without the concrete
/// peripheral type (which varies by pin assignment).
pub struct St7789Driver {
    /// Data/command select pin: HIGH = data, LOW = command.
    dc: Output<'static>,
    /// Active-low reset pin.
    rst: Output<'static>,
    /// Active-low chip-select pin.
    cs: Output<'static>,
    // spi: Spi<'static, esp_hal::Blocking>,  // wired in the real build
}

impl St7789Driver {
    /// Construct a new driver and perform the hardware initialisation sequence.
    ///
    /// # Arguments
    /// * `dc`  – data/command GPIO (output).
    /// * `rst` – active-low reset GPIO (output).
    /// * `cs`  – active-low chip-select GPIO (output).
    pub fn new(
        dc: Output<'static>,
        rst: Output<'static>,
        cs: Output<'static>,
    ) -> Self {
        let mut driver = Self { dc, rst, cs };
        driver.hard_reset();
        driver.init_sequence();
        driver
    }

    // ---- Initialisation -------------------------------------------------

    fn hard_reset(&mut self) {
        self.rst.set_low();
        // ~10 ms reset pulse at ~80 MHz CPU (200 000 × ~5 ns ≈ 10 ms).
        // Blocking busy-wait is acceptable here because this runs once at boot.
        for _ in 0..200_000u32 {
            core::hint::spin_loop();
        }
        self.rst.set_high();
        // Allow ≥5 ms for the panel to exit reset before sending commands.
        for _ in 0..200_000u32 {
            core::hint::spin_loop();
        }
    }

    fn init_sequence(&mut self) {
        // Software reset + sleep-out.
        self.write_command(cmd::SWRESET);
        self.delay_ms(150);

        self.write_command(cmd::SLPOUT);
        self.delay_ms(10);

        // 16-bit colour (RGB565).
        self.write_command(cmd::COLMOD);
        self.write_data(&[0x55]);

        // Memory data access control: MX + MV for landscape if needed.
        self.write_command(cmd::MADCTL);
        self.write_data(&[0x00]);

        // Normal display mode on.
        self.write_command(cmd::NORON);
        self.delay_ms(10);

        // Inversion on (most ST7789 modules need this for correct colours).
        self.write_command(cmd::INVON);

        // Display on.
        self.write_command(cmd::DISPON);
        self.delay_ms(10);
    }

    // ---- Drawing API ----------------------------------------------------

    /// Set the active pixel window (column / row address ranges).
    pub fn set_address_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) {
        self.write_command(cmd::CASET);
        self.write_data(&[
            (x0 >> 8) as u8, (x0 & 0xFF) as u8,
            (x1 >> 8) as u8, (x1 & 0xFF) as u8,
        ]);

        self.write_command(cmd::RASET);
        self.write_data(&[
            (y0 >> 8) as u8, (y0 & 0xFF) as u8,
            (y1 >> 8) as u8, (y1 & 0xFF) as u8,
        ]);

        self.write_command(cmd::RAMWR);
    }

    /// Flush a full-screen RGB565 framebuffer to the display.
    ///
    /// `buf` must be exactly `WIDTH * HEIGHT * 2` bytes.
    ///
    /// In a real build this would use SPI DMA to avoid blocking the CPU.
    pub fn flush(&mut self, buf: &[u8]) {
        self.set_address_window(0, 0, WIDTH - 1, HEIGHT - 1);
        self.cs.set_low();
        self.dc.set_high();
        // spi.write(buf).ok();  // real SPI transfer
        let _ = buf;
        self.cs.set_high();
    }

    // ---- Low-level SPI helpers ------------------------------------------

    fn write_command(&mut self, cmd: u8) {
        self.cs.set_low();
        self.dc.set_low();
        // spi.write(&[cmd]).ok();
        let _ = cmd;
        self.cs.set_high();
    }

    fn write_data(&mut self, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_high();
        // spi.write(data).ok();
        let _ = data;
        self.cs.set_high();
    }

    fn delay_ms(&self, ms: u32) {
        for _ in 0..(ms * 20_000) {
            core::hint::spin_loop();
        }
    }
}
