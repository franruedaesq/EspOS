//! # EspOS – Entry Point
//!
//! Initialises all hardware peripherals, sets up the two heap regions, then
//! spawns every Embassy task before handing control to the agentic state
//! machine loop.
//!
//! ## Core assignment
//! | Core | Tasks                                    |
//! |------|------------------------------------------|
//! | 0    | heartbeat, WiFi, state machine, telemetry, CLI |
//! | 1    | IMU sensor fusion, motor PWM             |
//!
//! ## Boot sequence
//! 1. `esp_hal::init` – clock tree, GPIO matrix, etc.
//! 2. [`memory::init_heap`] – register 512 KB SRAM with the global allocator.
//! 3. [`memory::init_psram!`] – register 8 MB PSRAM with the global allocator.
//! 4. Embassy timer init (`esp_hal_embassy::init` with SYSTIMER alarm0).
//! 5. Hardware watchdog enable (TIMG1 MWDT, 10-second timeout).
//! 6. Spawn all async tasks.
//! 7. Enter [`state_machine::run`] – never returns.

#![no_std]
#![no_main]

extern crate alloc;

// Bring in the panic handler + exception handler from esp-backtrace.
use esp_backtrace as _;

use embassy_executor::Spawner;
use esp_hal::{
    gpio::{Level, Output},
    timer::systimer::SystemTimer,
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
    // Use direct println before anything else to verify boot
    esp_println::println!("\\n\\n=== EspOS Starting ===");

    // -------------------------------------------------------------------------
    // 1. Initialise ESP32-S3 hardware
    // -------------------------------------------------------------------------
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_println::println!("=== HAL initialized ===");

    // Disable RTC watchdog immediately to prevent boot resets
    let mut rtc = esp_hal::rtc_cntl::Rtc::new(peripherals.LPWR);
    rtc.rwdt.disable();

    // Use direct println before logger init to verify UART works
    esp_println::println!("=== EspOS Boot Stage 1 ===");

    // -------------------------------------------------------------------------
    // 2. Heap allocators
    //    SRAM first (needed before any alloc), then PSRAM.
    // -------------------------------------------------------------------------
    memory::init_heap();
    esp_println::println!("=== Heap initialized ===");
    // Enable 8 MB octal PSRAM and register it with the allocator.
    // Uncomment when PSRAM is physically present:
    // memory::init_psram!(peripherals.PSRAM);

    // -------------------------------------------------------------------------
    // 3. Logging (over UART0 via esp-println)
    // -------------------------------------------------------------------------
    esp_println::println!("=== Initializing logger ===");
    esp_println::logger::init_logger(log::LevelFilter::Info);
    esp_println::println!("=== Logger initialized, testing info macro ===");
    info!("EspOS v{} booting on ESP32-S3...", env!("CARGO_PKG_VERSION"));
    esp_println::println!("=== Info macro works ===");

    // -------------------------------------------------------------------------
    // 4. Embassy timer back-end
    //    ESP32-S3: use SYSTIMER alarm0 as the Embassy time source.
    //    Note: ESP_HAL_EMBASSY_CONFIG_LOW_POWER_WAIT=false must be set in
    //    .cargo/config.toml \u2014 the default `waiti` sleep is incompatible with
    //    Wokwi's Xtensa simulator.
    // -------------------------------------------------------------------------
    esp_println::println!("=== Initializing Embassy timer ===");
    let systimer = SystemTimer::new(peripherals.SYSTIMER);
    esp_println::println!("=== SYSTIMER created ===");
    esp_hal_embassy::init(systimer.alarm0);
    esp_println::println!("=== Embassy timer initialized ===");

    // -------------------------------------------------------------------------
    // 5. Hardware watchdog (TIMG1) - DISABLED for Wokwi
    //    In Wokwi simulation, watchdogs can cause boot loops because tasks
    //    don't start running until after main() returns to the executor.
    //    On real hardware, re-enable this and feed it from the heartbeat task.
    // -------------------------------------------------------------------------
    esp_println::println!("=== Disabling hardware watchdog ===");
    let timg1 = TimerGroup::new(peripherals.TIMG1);
    let mut wdt1 = timg1.wdt;
    wdt1.disable();
    esp_println::println!("=== Hardware watchdog disabled ===");

    // -------------------------------------------------------------------------
    // 6. Peripheral setup
    // -------------------------------------------------------------------------

    // -- Heartbeat LED --------------------------------------------------------
    // GPIO 48 is the on-board RGB LED on most ESP32-S3 DevKitC boards.
    // Adjust to match your hardware.
    let led = Output::new(peripherals.GPIO48, Level::Low);

    // Spawn heartbeat for LED blinking
    esp_println::println!("=== Spawning heartbeat task ===");
    spawner
        .spawn(tasks::heartbeat::heartbeat_task(led, wdt1))
        .expect("spawn heartbeat_task");
    esp_println::println!("=== Heartbeat task spawned ===");

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

    // -- SSD1306 I2C display (Wokwi Configuration) ----------------------------
    // DISABLED - Running without display/joystick
    // esp_println::println!("=== Setting up I2C for SSD1306 ===");
    // use esp_hal::i2c::master::{Config as I2cConfig, I2c};
    // use esp_hal::time::RateExtU32;

    // let mut i2c_cfg = I2cConfig::default();
    // i2c_cfg.frequency = 400.kHz();

    // let i2c = I2c::new(peripherals.I2C0, i2c_cfg)
    //     .unwrap()
    //     .with_sda(peripherals.GPIO8)
    //     .with_scl(peripherals.GPIO9);
    // esp_println::println!("=== I2C and pins configured ===");

    // -------------------------------------------------------------------------
    // 7. WiFi + Network Stack Setup
    // -------------------------------------------------------------------------
    esp_println::println!("=== Initializing WiFi ===");

    use esp_wifi::wifi::WifiStaDevice;
    use embassy_net::{Config as NetConfig, StackResources};

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let wifi_init = esp_wifi::init(
        timg0.timer0,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
    ).unwrap();

    use static_cell::StaticCell;
    static WIFI_INIT: StaticCell<esp_wifi::EspWifiController<'static>> = StaticCell::new();
    let wifi_init = WIFI_INIT.init(wifi_init);

    let (wifi_interface, wifi_controller) = esp_wifi::wifi::new_with_mode(
        wifi_init,
        peripherals.WIFI,
        WifiStaDevice,
    ).unwrap();

    let net_config = NetConfig::dhcpv4(Default::default());

    static STACK_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    static STACK: StaticCell<embassy_net::Stack<'static>> = StaticCell::new();

    let (stack, runner) = embassy_net::new(
        wifi_interface,
        net_config,
        STACK_RESOURCES.init(StackResources::new()),
        embassy_time::Instant::now().as_ticks(),
    );

    let stack = &*STACK.init(stack);

    esp_println::println!("=== WiFi initialized, spawning tasks ===");
    spawner.spawn(tasks::wifi::net_task(runner)).expect("spawn net_task");
    spawner.spawn(tasks::wifi::wifi_task(wifi_controller, *stack)).expect("spawn wifi_task");
    spawner.spawn(tasks::web_server::web_server_task(stack)).expect("spawn web_server_task");
    esp_println::println!("=== WiFi and web server tasks spawned ===");

    // -------------------------------------------------------------------------
    // 8. Spawn remaining tasks
    // -------------------------------------------------------------------------

    // Telemetry: sample RAM, CPU load, and battery voltage every 1 second.
    spawner
        .spawn(tasks::telemetry::telemetry_task())
        .expect("spawn telemetry_task");

    // Debug CLI: process serial commands from the CLI_COMMAND_CHANNEL.
    // Wire a UART reader task (or an ISR) to push bytes via
    // `tasks::cli::push_byte(byte)` to feed the channel.
    spawner
        .spawn(tasks::cli::cli_task())
        .expect("spawn cli_task");

    // -------------------------------------------------------------------------
    // 9. Spawn tasks – Core 1
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
    // 10. Spawn tasks – any core
    // -------------------------------------------------------------------------

    // CAN bus / TWAI message router.
    spawner
        .spawn(tasks::can_bus::can_bus_task())
        .expect("spawn can_bus_task");

    // UI task - DISABLED - Running without display/joystick
    // esp_println::println!("=== Spawning UI task ===");
    // spawner
    //     .spawn(tasks::ui::ui_task(
    //         i2c,
    //         peripherals.ADC1,
    //         peripherals.GPIO1,
    //         peripherals.GPIO2,
    //     ))
    //     .expect("spawn ui_task");
    // esp_println::println!("=== UI task spawned ===");

    // -------------------------------------------------------------------------
    // 11. Agentic state machine (runs on this task, Core 0)
    // -------------------------------------------------------------------------
    esp_println::println!("=== All tasks spawned, entering state machine ===");
    state_machine::run(&spawner).await;

    // Unreachable – state_machine::run loops forever.
    unreachable!()
}
