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
use esp_hal::analog::adc::{Adc, AdcConfig, Attenuation};
use esp_hal::gpio::GpioPin;
use esp_hal::peripherals::ADC1;

use embedded_graphics::{
    mono_font::{ascii::{FONT_5X8, FONT_6X10}, MonoTextStyle},
    pixelcolor::{BinaryColor, Rgb565},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::Text,
};

// ---------------------------------------------------------------------------
// Display constants
// ---------------------------------------------------------------------------

/// Horizontal resolution of the SSD1306 display in pixels (Wokwi).
pub const DISPLAY_WIDTH: usize = 128;
/// Vertical resolution of the SSD1306 display in pixels (Wokwi).
pub const DISPLAY_HEIGHT: usize = 64;
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

/// Face mood state for animations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaceMood {
    Happy,
    Neutral,
    Thinking,
    Excited,
    Love,
    Mad,
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

fn render_scene<D>(display: &mut D, mood: FaceMood, frame: u32, eye_dx: i32, eye_dy: i32)
where
    D: DrawTarget<Color = BinaryColor, Error = core::convert::Infallible>,
{
    let _ = display.clear(BinaryColor::Off);
    let fill = PrimitiveStyle::with_fill(BinaryColor::On);
    let stroke = PrimitiveStyle::with_stroke(BinaryColor::On, 1);

    // Face centered, minimal "screen bot" style
    let face_cx = 64;
    let face_cy = 30;

    // Subtle bobbing
    let bob = match (frame / 10) % 4 {
        0 => 0,
        1 => 1,
        2 => 0,
        _ => -1,
    };

    let eye_center_y = face_cy - 10 + bob + eye_dy;
    let left_eye_x = face_cx - 18 + eye_dx;
    let right_eye_x = face_cx + 18 + eye_dx;

    // Blink + occasional wink
    let blink = frame % 84;
    let both_open = blink < 76;
    let wink_phase = frame % 280;
    let right_wink = wink_phase > 220 && wink_phase < 236;

    // Rounded-vertical eye shape helper (drawn inline with circles + rectangle)
    let eye_w = 16;
    let eye_h = 26;
    let eye_r = eye_w / 2;

    let draw_open_eye = |display: &mut D, center_x: i32| {
        let x = center_x - eye_w / 2;
        let y = eye_center_y - eye_h / 2;

        Rectangle::new(
            Point::new(x, y + eye_r),
            Size::new(eye_w as u32, (eye_h - eye_w) as u32),
        )
        .into_styled(fill)
        .draw(display)
        .ok();

        Circle::new(Point::new(x, y), eye_w as u32)
            .into_styled(fill)
            .draw(display)
            .ok();
        Circle::new(Point::new(x, y + eye_h - eye_w), eye_w as u32)
            .into_styled(fill)
            .draw(display)
            .ok();
    };

    let draw_closed_eye = |display: &mut D, center_x: i32| {
        Line::new(
            Point::new(center_x - 8, eye_center_y + 1),
            Point::new(center_x + 8, eye_center_y + 1),
        )
        .into_styled(stroke)
        .draw(display)
        .ok();
    };

    if both_open {
        draw_open_eye(display, left_eye_x);
        if right_wink {
            draw_closed_eye(display, right_eye_x);
        } else {
            draw_open_eye(display, right_eye_x);
        }
    } else {
        draw_closed_eye(display, left_eye_x);
        draw_closed_eye(display, right_eye_x);
    }

    // Optional mood overlays while keeping same base style
    if mood == FaceMood::Love {
        for &x in &[left_eye_x, right_eye_x] {
            Circle::new(Point::new(x - 5, eye_center_y - 6), 5)
                .into_styled(fill)
                .draw(display)
                .ok();
            Circle::new(Point::new(x, eye_center_y - 6), 5)
                .into_styled(fill)
                .draw(display)
                .ok();
            Line::new(Point::new(x - 7, eye_center_y - 3), Point::new(x - 2, eye_center_y + 4))
                .into_styled(fill)
                .draw(display)
                .ok();
            Line::new(Point::new(x + 2, eye_center_y - 3), Point::new(x - 2, eye_center_y + 4))
                .into_styled(fill)
                .draw(display)
                .ok();
        }
    }

    if mood == FaceMood::Mad {
        Line::new(
            Point::new(left_eye_x - 9, eye_center_y - 10),
            Point::new(left_eye_x + 6, eye_center_y - 14),
        )
        .into_styled(stroke)
        .draw(display)
        .ok();
        Line::new(
            Point::new(right_eye_x - 6, eye_center_y - 14),
            Point::new(right_eye_x + 9, eye_center_y - 10),
        )
        .into_styled(stroke)
        .draw(display)
        .ok();
    }

    // Small curved smile similar to reference image
    let mouth_y = face_cy + 12 + bob;
    let mouth_width = if (frame / 18).is_multiple_of(2) { 18 } else { 16 };
    let x0 = face_cx - mouth_width / 2;
    let x1 = face_cx + mouth_width / 2;

    Line::new(Point::new(x0, mouth_y), Point::new(x0 + 4, mouth_y + 4))
        .into_styled(stroke)
        .draw(display)
        .ok();
    Line::new(
        Point::new(x0 + 4, mouth_y + 4),
        Point::new(x1 - 4, mouth_y + 4),
    )
    .into_styled(stroke)
    .draw(display)
    .ok();
    Line::new(Point::new(x1 - 4, mouth_y + 4), Point::new(x1, mouth_y))
        .into_styled(stroke)
        .draw(display)
        .ok();

    if mood == FaceMood::Mad {
        // Flatten smile when mad
        Rectangle::new(Point::new(x0 - 1, mouth_y + 2), Size::new((mouth_width + 2) as u32, 5))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(display)
            .ok();
        Line::new(Point::new(x0 + 1, mouth_y + 2), Point::new(x1 - 1, mouth_y + 2))
            .into_styled(stroke)
            .draw(display)
            .ok();
    }

    // Brand label – bottom-right
    let style = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
    Text::new("EspOS", Point::new(96, 63), style)
        .draw(display)
        .ok();

    // Live stats – bottom-left (FONT_5X8: 5px wide, 8px tall)
    // Line 1 (y=56): free RAM in KB
    // Line 2 (y=63): CPU %
    let small = MonoTextStyle::new(&FONT_5X8, BinaryColor::On);
    let free_kb = crate::memory::heap_free() / 1024;
    let cpu = crate::tasks::telemetry::LAST_CPU_PERCENT.load(
        core::sync::atomic::Ordering::Relaxed,
    );

    let mut ram_str: heapless::String<12> = heapless::String::new();
    let _ = core::fmt::write(&mut ram_str, format_args!("R:{free_kb}K", free_kb = free_kb));
    Text::new(ram_str.as_str(), Point::new(0, 56), small)
        .draw(display)
        .ok();

    let mut cpu_str: heapless::String<8> = heapless::String::new();
    let _ = core::fmt::write(&mut cpu_str, format_args!("C:{cpu}%", cpu = cpu));
    Text::new(cpu_str.as_str(), Point::new(0, 63), small)
        .draw(display)
        .ok();
}
/// Apply a single [`UiDrawCommand`] to the display.
fn compose(cmd: &UiDrawCommand, mood: &mut FaceMood) {
    match cmd {
        UiDrawCommand::Clear(color) => {
            *mood = if *color == Rgb565::BLACK {
                FaceMood::Neutral
            } else {
                FaceMood::Excited
            };
        }
        UiDrawCommand::StatusText(_) => *mood = FaceMood::Happy,
        UiDrawCommand::ShowState(_) => *mood = FaceMood::Thinking,
        UiDrawCommand::ProgressBar { percent, .. } => {
            *mood = if *percent >= 90 {
                FaceMood::Love
            } else if *percent <= 15 {
                FaceMood::Mad
            } else if *percent >= 70 {
                FaceMood::Excited
            } else if *percent >= 50 {
                FaceMood::Happy
            } else {
                FaceMood::Neutral
            };
        }
    }
}

fn axis_to_offset(raw: u16) -> i32 {
    let delta = raw as i32 - 2048;
    if delta.abs() < 180 {
        0
    } else {
        (delta * 6 / 2048).clamp(-6, 6)
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
pub async fn ui_task(
    i2c: esp_hal::i2c::master::I2c<'static, esp_hal::Blocking>,
    adc1: ADC1,
    gpio1: GpioPin<1>,
    gpio2: GpioPin<2>,
) {
    esp_println::println!("[ui] Task started – {}x{} @ {} fps", DISPLAY_WIDTH, DISPLAY_HEIGHT, TARGET_FPS);

    // Initialize SSD1306 driver
    esp_println::println!("[ui] Initializing SSD1306 driver...");
    let mut display = crate::drivers::ssd1306::Ssd1306Driver::new(i2c);
    esp_println::println!("[ui] Display driver initialized!");

    // Joystick ADC channels (Wokwi: HORZ->GPIO1, VERT->GPIO2)
    let mut adc_config = AdcConfig::new();
    let mut joy_x = adc_config.enable_pin(gpio1, Attenuation::_11dB);
    let mut joy_y = adc_config.enable_pin(gpio2, Attenuation::_11dB);
    let mut adc = Adc::new(adc1, adc_config);

    esp_println::println!("[ui] Initial screen drawn, entering main loop");

    // Animation state
    let mut frame: u32 = 0;
    let mut mood = FaceMood::Happy;
    let mut eye_dx: i32 = 0;
    let mut eye_dy: i32 = 0;
    loop {
        // Wait for a draw command with timeout
        match embassy_time::with_timeout(
            embassy_time::Duration::from_millis(FRAME_PERIOD_MS),
            UI_DRAW_CHANNEL.receive()
        ).await {
            Ok(cmd) => {
                log::info!("[ui] Received command: {:?}", cmd);
                compose(&cmd, &mut mood);

                // Drain any additional pending commands (batch processing)
                while let Ok(cmd) = UI_DRAW_CHANNEL.try_receive() {
                    log::debug!("[ui] Batch command: {:?}", cmd);
                    compose(&cmd, &mut mood);
                }
            }
            Err(_timeout) => {
                // No commands in this frame, keep current mood.
            }
        }

        let raw_x = adc.read_blocking(&mut joy_x);
        let raw_y = adc.read_blocking(&mut joy_y);

        let target_dx = axis_to_offset(raw_x);
        let target_dy = -axis_to_offset(raw_y);
        eye_dx = (eye_dx * 3 + target_dx) / 4;
        eye_dy = (eye_dy * 3 + target_dy) / 4;

        render_scene(&mut display, mood, frame, eye_dx, eye_dy);
        let _ = display.flush();

        // Update animation frame counter
        frame = frame.wrapping_add(1);

        // Idle mood animation cycle with occasional Love/Mad expressions.
        mood = match (frame / 72) % 12 {
            0 | 1 | 2 => FaceMood::Happy,
            3 | 4 => FaceMood::Neutral,
            5 | 6 => FaceMood::Thinking,
            7 | 8 => FaceMood::Excited,
            9 | 10 => FaceMood::Love,
            _ => FaceMood::Mad,
        };
    }
}
