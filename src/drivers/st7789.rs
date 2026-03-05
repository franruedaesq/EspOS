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
use esp_hal::spi::master::Spi;
use esp_hal::Blocking;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{raw::RawU16, Rgb565},
    Pixel,
    primitives::Rectangle,
    prelude::RawData,
};

pub const WIDTH: u16 = 320;
pub const HEIGHT: u16 = 240;

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

pub struct St7789Driver<'a> {
    dc: Output<'a>,
    rst: Output<'a>,
    cs: Output<'a>,
    spi: Spi<'a, Blocking>,
}

impl<'a> St7789Driver<'a> {
    pub fn new(
        dc: Output<'a>,
        rst: Output<'a>,
        cs: Output<'a>,
        spi: Spi<'a, Blocking>,
    ) -> Self {
        let mut driver = Self { dc, rst, cs, spi };
        driver.hard_reset();
        driver.init_sequence();
        driver
    }

    fn hard_reset(&mut self) {
        esp_println::println!("[st7789] Starting hard reset...");
        self.rst.set_low();
        // Keep reset low long enough for ILI9341 to latch reset.
        self.delay_ms(10);
        self.rst.set_high();
        // Wait after reset release before sending commands.
        self.delay_ms(120);
        esp_println::println!("[st7789] Hard reset complete");
    }

    fn init_sequence(&mut self) {
        esp_println::println!("[st7789] Starting init sequence...");
        self.write_command(cmd::SWRESET);
        esp_println::println!("[st7789] SWRESET sent");
        self.delay_ms(150);
        self.write_command(cmd::SLPOUT);
        esp_println::println!("[st7789] SLPOUT sent");
        // Critical: ILI9341 needs >=120ms after SLPOUT.
        self.delay_ms(120);
        // 16-bit RGB565 pixel format
        self.write_command(cmd::COLMOD);
        self.write_data(&[0x55]);
        esp_println::println!("[st7789] RGB565 mode set");
        self.delay_ms(10);
        // MADCTL tuned for Wokwi ILI9341 landscape.
        self.write_command(cmd::MADCTL);
        self.write_data(&[0x48]);
        self.write_command(cmd::INVOFF);
        self.write_command(cmd::NORON);
        esp_println::println!("[st7789] Display mode configured");
        self.delay_ms(10);
        self.write_command(cmd::DISPON);
        esp_println::println!("[st7789] Display ON");
        self.delay_ms(50);
        esp_println::println!("[st7789] Init sequence complete");
    }

    /// Sets the drawing window AND issues RAMWR.
    /// CS is left LOW so the caller can stream pixel data immediately.
    pub fn set_address_window(&mut self, x0: u16, y0: u16, x1: u16, y1: u16) {
        self.cs.set_low();

        self.dc.set_low();
        let _ = self.spi.write_bytes(&[cmd::CASET]);
        self.dc.set_high();
        let _ = self.spi.write_bytes(&[(x0 >> 8) as u8, (x0 & 0xFF) as u8, (x1 >> 8) as u8, (x1 & 0xFF) as u8]);

        self.dc.set_low();
        let _ = self.spi.write_bytes(&[cmd::RASET]);
        self.dc.set_high();
        let _ = self.spi.write_bytes(&[(y0 >> 8) as u8, (y0 & 0xFF) as u8, (y1 >> 8) as u8, (y1 & 0xFF) as u8]);

        self.dc.set_low();
        let _ = self.spi.write_bytes(&[cmd::RAMWR]);
        self.dc.set_high();
        // CS stays LOW — caller must set CS high when done writing pixels.
    }

    fn write_command(&mut self, cmd: u8) {
        self.cs.set_low();
        self.dc.set_low();
        let _ = self.spi.write_bytes(&[cmd]);
        self.cs.set_high();
    }

    fn write_data(&mut self, data: &[u8]) {
        self.cs.set_low();
        self.dc.set_high();
        let _ = self.spi.write_bytes(data);
        self.cs.set_high();
    }

    /// Write pixel data bytes while CS is already held LOW.
    fn write_data_continue(&mut self, data: &[u8]) {
        let _ = self.spi.write_bytes(data);
    }

    fn delay_ms(&self, ms: u32) {
        for _ in 0..(ms * 20_000) { core::hint::spin_loop(); }
    }
}

// ---- Implementación Gráfica (El traductor de píxeles) ----

impl<'a> OriginDimensions for St7789Driver<'a> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<'a> DrawTarget for St7789Driver<'a> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            if coord.x >= 0 && coord.x < WIDTH as i32 && coord.y >= 0 && coord.y < HEIGHT as i32 {
                // set_address_window leaves CS LOW with DC HIGH, ready for pixel data
                self.set_address_window(coord.x as u16, coord.y as u16, coord.x as u16, coord.y as u16);
                let raw = RawU16::from(color).into_inner();
                self.write_data_continue(&[(raw >> 8) as u8, (raw & 0xFF) as u8]);
                self.cs.set_high();
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        if area.size.width == 0 || area.size.height == 0 {
            return Ok(());
        }

        let x0 = area.top_left.x.max(0) as u16;
        let y0 = area.top_left.y.max(0) as u16;
        let x1 = (area.top_left.x + area.size.width as i32 - 1).min((WIDTH - 1) as i32) as u16;
        let y1 = (area.top_left.y + area.size.height as i32 - 1).min((HEIGHT - 1) as i32) as u16;

        let pixel_count = (x1 - x0 + 1) as usize * (y1 - y0 + 1) as usize;

        self.set_address_window(x0, y0, x1, y1);

        for color in colors.into_iter().take(pixel_count) {
            let raw = RawU16::from(color).into_inner();
            self.write_data_continue(&[(raw >> 8) as u8, (raw & 0xFF) as u8]);
        }

        self.cs.set_high();
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        if area.size.width == 0 || area.size.height == 0 {
            return Ok(());
        }

        let x0 = area.top_left.x.max(0) as u16;
        let y0 = area.top_left.y.max(0) as u16;
        let x1 = (area.top_left.x + area.size.width as i32 - 1).min((WIDTH - 1) as i32) as u16;
        let y1 = (area.top_left.y + area.size.height as i32 - 1).min((HEIGHT - 1) as i32) as u16;

        let pixel_count = (x1 - x0 + 1) as usize * (y1 - y0 + 1) as usize;
        let raw = RawU16::from(color).into_inner();
        let bytes = [(raw >> 8) as u8, (raw & 0xFF) as u8];

        self.set_address_window(x0, y0, x1, y1);
        for _ in 0..pixel_count {
            self.write_data_continue(&bytes);
        }
        self.cs.set_high();
        Ok(())
    }
}
