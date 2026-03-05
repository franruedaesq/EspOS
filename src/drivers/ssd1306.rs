//! # SSD1306 I2C Display Driver
//!
//! Minimal SSD1306 128x64 monochrome driver for Wokwi and ESP32-S3.

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::BinaryColor,
    Pixel,
};
use esp_hal::{i2c::master::I2c, Blocking};

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
const PAGES: usize = HEIGHT / 8;
const BUFFER_SIZE: usize = WIDTH * PAGES;
const SSD1306_ADDR: u8 = 0x3C;

pub struct Ssd1306Driver<'d> {
    i2c: I2c<'d, Blocking>,
    buffer: [u8; BUFFER_SIZE],
}

impl<'d> Ssd1306Driver<'d> {
    pub fn new(i2c: I2c<'d, Blocking>) -> Self {
        let mut driver = Self {
            i2c,
            buffer: [0; BUFFER_SIZE],
        };
        driver.init();
        driver
    }

    fn init(&mut self) {
        esp_println::println!("[ssd1306] init start");
        self.cmd(0xAE); // Display OFF
        self.cmd(0xD5); // Set display clock div
        self.cmd(0x80);
        self.cmd(0xA8); // Multiplex
        self.cmd(0x3F); // 1/64
        self.cmd(0xD3); // Display offset
        self.cmd(0x00);
        self.cmd(0x40); // Start line = 0
        self.cmd(0x8D); // Charge pump
        self.cmd(0x14); // Enable
        self.cmd(0x20); // Memory mode
        self.cmd(0x00); // Horizontal addressing
        self.cmd(0xA1); // Segment remap
        self.cmd(0xC8); // COM scan dec
        self.cmd(0xDA); // COM pins
        self.cmd(0x12);
        self.cmd(0x81); // Contrast
        self.cmd(0x7F);
        self.cmd(0xD9); // Precharge
        self.cmd(0xF1);
        self.cmd(0xDB); // VCOM detect
        self.cmd(0x40);
        self.cmd(0xA4); // Resume RAM content display
        self.cmd(0xA6); // Normal display
        self.cmd(0x2E); // Deactivate scroll
        self.cmd(0xAF); // Display ON
        self.clear(BinaryColor::Off);
        let _ = self.flush();
        esp_println::println!("[ssd1306] init done");
    }

    fn cmd(&mut self, cmd: u8) {
        let _ = self.i2c.write(SSD1306_ADDR, &[0x00, cmd]);
    }

    pub fn flush(&mut self) -> Result<(), ()> {
        self.cmd(0x21); // Set column address
        self.cmd(0x00);
        self.cmd((WIDTH as u8) - 1);
        self.cmd(0x22); // Set page address
        self.cmd(0x00);
        self.cmd((PAGES as u8) - 1);

        let mut packet = [0u8; 17];
        packet[0] = 0x40;

        for chunk in self.buffer.chunks(16) {
            packet[1..(1 + chunk.len())].copy_from_slice(chunk);
            self.i2c
                .write(SSD1306_ADDR, &packet[..(1 + chunk.len())])
                .map_err(|_| ())?;
        }

        Ok(())
    }
}

impl<'d> OriginDimensions for Ssd1306Driver<'d> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl<'d> DrawTarget for Ssd1306Driver<'d> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels {
            if coord.x < 0 || coord.y < 0 {
                continue;
            }
            let x = coord.x as usize;
            let y = coord.y as usize;
            if x >= WIDTH || y >= HEIGHT {
                continue;
            }

            let byte_index = x + (y / 8) * WIDTH;
            let bit = 1u8 << (y % 8);
            match color {
                BinaryColor::On => self.buffer[byte_index] |= bit,
                BinaryColor::Off => self.buffer[byte_index] &= !bit,
            }
        }
        Ok(())
    }
}
