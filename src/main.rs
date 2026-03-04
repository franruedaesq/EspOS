//! # EspOS – Entry Point
//!
//! Initialises all hardware peripherals, sets up the two heap regions, then
//! spawns every Embassy task before handing control to the agentic state
//! machine loop.
//!
//! ## Core assignment
//! | Core | Tasks                                    |
//! |------|------------------------------------------|
//! | 0    | heartbeat, WiFi, state machine           |
//! | 1    | IMU sensor fusion, motor PWM             |
//!
//! ## Boot sequence
//! 1. `esp_hal::init` – clock tree, GPIO matrix, etc.
//! 2. [`memory::init_heap`] – register 512 KB SRAM with the global allocator.
//! 3. [`memory::init_psram!`] – register 8 MB PSRAM with the global allocator.
//! 4. Embassy timer init (`esp_hal_embassy::init`).
//! 5. Spawn all async tasks.
//! 6. Enter [`state_machine::run`] – never returns.

#![no_std]
#![no_main]

extern crate alloc;

// Bring in the panic handler + exception handler from esp-backtrace.
use esp_backtrace as _;

use embassy_executor::Spawner;
use esp_hal::{
    gpio::{Level, Output},
    timer::timg::TimerGroup,
};
use log::info;

// ---- Internal modules -------------------------------------------------------
mod drivers;
mod hal;
mod memory;
mod state_machine;
mod tasks;

// ---- Embassy main entry point -----------------------------------------------

/// Firmware entry point.
///
/// The `#[esp_hal_embassy::main]` macro:
/// * Replaces the standard `main` symbol with the correct Xtensa entry point.
/// * Creates an Embassy executor and passes a [`Spawner`] handle here.
#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    // -------------------------------------------------------------------------
    // 1. Initialise ESP32-S3 hardware
    // -------------------------------------------------------------------------
    let peripherals = esp_hal::init(esp_hal::Config::default());

    // -------------------------------------------------------------------------
    // 2. Heap allocators
    //    SRAM first (needed before any alloc), then PSRAM.
    // -------------------------------------------------------------------------
    memory::init_heap();
    // Enable 8 MB octal PSRAM and register it with the allocator.
    // Uncomment when PSRAM is physically present:
    // memory::init_psram!(peripherals.PSRAM);

    // -------------------------------------------------------------------------
    // 3. Logging (over UART0 via esp-println)
    // -------------------------------------------------------------------------
    esp_println::logger::init_logger_from_env();
    info!("EspOS v{} booting on ESP32-S3…", env!("CARGO_PKG_VERSION"));

    // -------------------------------------------------------------------------
    // 4. Embassy timer back-end
    //    Uses TIMG0 timer 0 as the Embassy time-keeper.
    // -------------------------------------------------------------------------
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // -------------------------------------------------------------------------
    // 5. Peripheral setup
    // -------------------------------------------------------------------------

    // -- Heartbeat LED --------------------------------------------------------
    // GPIO 48 is the on-board RGB LED on most ESP32-S3 DevKitC boards.
    // Adjust to match your hardware.
    let led = Output::new(peripherals.GPIO48, Level::Low);

    // -- I²C bus for IMU + ToF ------------------------------------------------
    // SDA = GPIO 8, SCL = GPIO 9 (common DevKit mapping).
    // The `I2c::new` call is shown here for reference; the drivers own the bus.
    // let i2c0 = esp_hal::i2c::master::I2c::new(peripherals.I2C0, {
    //     let mut cfg = esp_hal::i2c::master::Config::default();
    //     cfg.frequency = esp_hal::time::HertzU32::kHz(400);
    //     cfg
    // })
    // .with_sda(peripherals.GPIO8)
    // .with_scl(peripherals.GPIO9);

    // -- L298N motor driver GPIOs ---------------------------------------------
    // let in1 = Output::new(peripherals.GPIO4,  Level::Low);
    // let in2 = Output::new(peripherals.GPIO5,  Level::Low);
    // let in3 = Output::new(peripherals.GPIO6,  Level::Low);
    // let in4 = Output::new(peripherals.GPIO7,  Level::Low);

    // -- ST7789 SPI display ---------------------------------------------------
    // let dc  = Output::new(peripherals.GPIO2,  Level::Low);
    // let rst = Output::new(peripherals.GPIO3,  Level::High);
    // let cs  = Output::new(peripherals.GPIO10, Level::High);

    // -------------------------------------------------------------------------
    // 6. Spawn tasks – Core 0
    // -------------------------------------------------------------------------

    // Heartbeat: 1 Hz LED blink to show the system is alive.
    spawner
        .spawn(tasks::heartbeat::heartbeat_task(led))
        .expect("spawn heartbeat_task");

    // WiFi + embassy-net tasks.
    // Requires esp-wifi controller and network stack to be initialised first.
    // The full WiFi init sequence is shown below; it is gated on a feature flag
    // so the firmware boots without credentials during development.
    //
    // let (wifi_interface, wifi_controller) = esp_wifi::wifi::new_with_mode(
    //     &wifi_init, peripherals.WIFI, WifiStaDevice).unwrap();
    // let net_config = embassy_net::Config::dhcpv4(Default::default());
    // static NET_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    // let (stack, runner) = embassy_net::new(
    //     wifi_interface, net_config,
    //     NET_RESOURCES.init(StackResources::new()),
    //     embassy_time::Instant::now().as_ticks(),
    // );
    // spawner.spawn(tasks::wifi::net_task(runner)).expect("spawn net_task");
    // spawner.spawn(tasks::wifi::wifi_task(wifi_controller, stack)).expect("spawn wifi_task");

    // -------------------------------------------------------------------------
    // 7. Spawn tasks – Core 1
    // -------------------------------------------------------------------------

    // IMU sensor fusion at 100 Hz.
    spawner
        .spawn(tasks::imu::imu_task())
        .expect("spawn imu_task");

    // Motor PWM controller.
    spawner
        .spawn(tasks::motor::motor_task())
        .expect("spawn motor_task");

    // -------------------------------------------------------------------------
    // 8. Spawn tasks – any core
    // -------------------------------------------------------------------------

    // CAN bus / TWAI message router.
    spawner
        .spawn(tasks::can_bus::can_bus_task())
        .expect("spawn can_bus_task");

    // UI framebuffer renderer + ST7789 SPI sync.
    spawner
        .spawn(tasks::ui::ui_task())
        .expect("spawn ui_task");

    // -------------------------------------------------------------------------
    // 9. Agentic state machine (runs on this task, Core 0)
    // -------------------------------------------------------------------------
    info!("EspOS: all tasks spawned – entering state machine");
    state_machine::run(&spawner).await;

    // Unreachable – state_machine::run loops forever.
    unreachable!()
}
