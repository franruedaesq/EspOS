//! # WiFi Task
//!
//! Connects the ESP32-S3 to a WiFi access point using `esp-wifi` and
//! `embassy-net` with DHCP.  Once an IP address is obtained it is logged over
//! serial and the address is published on a static channel so other tasks can
//! open TCP/UDP sockets.
//!
//! ## Architecture
//! ```text
//!  wifi_task  ──┬──► embassy-net Stack (DHCP)
//!               └──► IP_CHANNEL  ──► state_machine
//! ```

use core::str::FromStr;

use embassy_executor::task;
use embassy_net::{Config, Runner, Stack, StackResources};
use embassy_time::Timer;
use esp_wifi::wifi::{
    ClientConfiguration, Configuration, WifiController, WifiDevice, WifiEvent, WifiStaDevice,
    WifiState,
};
use static_cell::StaticCell;

// ---------------------------------------------------------------------------
// Compile-time WiFi credentials
// ---------------------------------------------------------------------------

/// Target SSID – override via environment variable at build time.
const SSID: &str = env!("WIFI_SSID");

/// WPA2 passphrase – override via environment variable at build time.
const PASSWORD: &str = env!("WIFI_PASSWORD");

// ---------------------------------------------------------------------------
// Static allocations required by embassy-net
// ---------------------------------------------------------------------------

static STACK_RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();

// ---------------------------------------------------------------------------
// Inter-task IP address channel
// ---------------------------------------------------------------------------

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use heapless::String;

/// Capacity: a single IPv4 address string (`"xxx.xxx.xxx.xxx\0"` ≤ 16 bytes).
pub static IP_CHANNEL: Channel<CriticalSectionRawMutex, String<16>, 1> = Channel::new();

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

/// Embassy task that owns the `embassy-net` runner and drives the network
/// stack forward.
///
/// This task must be spawned **alongside** [`wifi_connection_task`].
#[task]
pub async fn net_task(mut runner: Runner<'static, WifiDevice<'static, WifiStaDevice>>) {
    runner.run().await
}

/// Embassy task that manages the WiFi connection life-cycle:
///
/// 1. Configures the WiFi interface in station mode.
/// 2. Initiates association with [`SSID`] / [`PASSWORD`].
/// 3. Waits for DHCP to assign an IPv4 address.
/// 4. Publishes the address on [`IP_CHANNEL`].
/// 5. Monitors the link and reconnects on disconnect.
#[task]
pub async fn wifi_task(
    controller: WifiController<'static>,
    stack: Stack<'static>,
) {
    log::info!("[wifi] task started");

    // ---- associate -------------------------------------------------------
    // Validate credential lengths before constructing the configuration.
    // SSID max = 32 chars, password max = 64 chars (WPA2 PSK limit).
    let ssid = match heapless::String::from_str(SSID) {
        Ok(s) => s,
        Err(_) => {
            log::error!("[wifi] SSID too long (max 32 chars) – task halted");
            return;
        }
    };
    let password = match heapless::String::from_str(PASSWORD) {
        Ok(p) => p,
        Err(_) => {
            log::error!("[wifi] PASSWORD too long (max 64 chars) – task halted");
            return;
        }
    };
    let client_cfg = ClientConfiguration {
        ssid,
        password,
        ..Default::default()
    };

    let mut ctrl = controller;
    if let Err(e) = ctrl.set_configuration(&Configuration::Client(client_cfg)) {
        log::error!("[wifi] failed to set client configuration: {:?}", e);
        return;
    }
    if let Err(e) = ctrl.start_async().await {
        log::error!("[wifi] failed to start WiFi controller: {:?}", e);
        return;
    }
    log::info!("[wifi] connecting to '{}'…", SSID);

    loop {
        match ctrl.connect_async().await {
            Ok(()) => log::info!("[wifi] associated"),
            Err(e) => {
                log::warn!("[wifi] connect error: {:?} – retrying in 5 s", e);
                Timer::after_millis(5_000).await;
                continue;
            }
        }

        // ---- wait for DHCP lease -----------------------------------------
        log::info!("[wifi] waiting for DHCP…");
        loop {
            if let Some(config) = stack.config_v4() {
                let mut addr: heapless::String<16> = heapless::String::new();
                let ip = config.address.address();
                // Format the IPv4 address into the heapless string.
                let _ = core::fmt::write(
                    &mut addr,
                    format_args!("{}.{}.{}.{}",
                        ip.as_bytes()[0], ip.as_bytes()[1],
                        ip.as_bytes()[2], ip.as_bytes()[3]),
                );
                log::info!("[wifi] IP: {}", addr.as_str());
                // Best-effort send; receiver may not be waiting yet.
                let _ = IP_CHANNEL.try_send(addr);
                break;
            }
            Timer::after_millis(500).await;
        }

        // ---- monitor link ------------------------------------------------
        loop {
            if matches!(ctrl.is_connected(), Ok(true)) {
                Timer::after_millis(2_000).await;
            } else {
                log::warn!("[wifi] disconnected – reconnecting…");
                break;
            }
        }
    }
}
