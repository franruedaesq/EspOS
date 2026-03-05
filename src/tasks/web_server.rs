//! # Web Server Task
//!
//! Lightweight HTTP server showing ESP32-S3 hardware status:
//! - WiFi connection state
//! - Dual-core processor load
//! - SRAM memory usage (128 KB)
//! - System uptime
//! - Heartbeat health
//!
//! Access via: http://<ESP32-IP>/

use embassy_executor::task;
use embassy_net::{Stack, tcp::TcpSocket};
use embassy_time::{Duration, Instant};
use embedded_io_async::Write;

use crate::memory;
use crate::tasks::telemetry::LAST_CPU_PERCENT;
use crate::tasks::heartbeat::HEARTBEAT_TICKS;

/// Boot time - captured when the web server starts
static mut BOOT_TIME: Option<Instant> = None;

/// HTTP response header for HTML content
const HTTP_HEADER: &str = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n";

/// HTTP response header for JSON content
const HTTP_JSON_HEADER: &str = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n";

/// HTML dashboard showing ESP32-S3 system status
const HTML_DASHBOARD: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>ESP32-S3 Dashboard</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', Tahoma, Geneva, Verdana, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            color: #fff;
            padding: 20px;
            min-height: 100vh;
        }
        .container { max-width: 800px; margin: 0 auto; }
        h1 {
            text-align: center;
            margin-bottom: 10px;
            font-size: 2.5em;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.3);
        }
        .subtitle {
            text-align: center;
            margin-bottom: 30px;
            opacity: 0.9;
            font-size: 1.1em;
        }
        .grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
            gap: 20px;
            margin-bottom: 20px;
        }
        .card {
            background: rgba(255, 255, 255, 0.1);
            backdrop-filter: blur(10px);
            border-radius: 15px;
            padding: 25px;
            box-shadow: 0 8px 32px 0 rgba(31, 38, 135, 0.37);
            border: 1px solid rgba(255, 255, 255, 0.18);
            transition: transform 0.3s ease;
        }
        .card:hover { transform: translateY(-5px); }
        .card-title {
            font-size: 0.9em;
            opacity: 0.8;
            margin-bottom: 10px;
            text-transform: uppercase;
            letter-spacing: 1px;
        }
        .card-value {
            font-size: 2.5em;
            font-weight: bold;
            margin-bottom: 5px;
            text-shadow: 2px 2px 4px rgba(0,0,0,0.2);
        }
        .card-unit { font-size: 0.9em; opacity: 0.7; }
        .status-online { color: #4ade80; font-weight: bold; }
        .status-offline { color: #f87171; }
        .footer {
            text-align: center;
            margin-top: 30px;
            opacity: 0.7;
            font-size: 0.9em;
        }
        .progress-bar {
            width: 100%;
            height: 10px;
            background: rgba(255,255,255,0.2);
            border-radius: 5px;
            overflow: hidden;
            margin-top: 10px;
        }
        .progress-fill {
            height: 100%;
            background: linear-gradient(90deg, #4ade80 0%, #22c55e 100%);
            transition: width 0.5s ease;
        }
        .chip-info {
            background: rgba(255, 255, 255, 0.15);
            padding: 15px;
            border-radius: 10px;
            margin-bottom: 20px;
            text-align: center;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 ESP32-S3</h1>
        <p class="subtitle">EspOS Hardware Monitor</p>

        <div class="chip-info">
            <strong>Xtensa LX7 Dual-Core</strong> @ 240 MHz |
            WiFi 802.11 b/g/n | Bluetooth 5 (LE)
        </div>

        <div class="grid">
            <div class="card">
                <div class="card-title">WiFi Status</div>
                <div class="card-value status-online" id="wifi-status">●</div>
                <div class="card-unit" id="wifi-ip">--</div>
            </div>

            <div class="card">
                <div class="card-title">System Uptime</div>
                <div class="card-value" id="uptime">--</div>
                <div class="card-unit">seconds</div>
            </div>

            <div class="card">
                <div class="card-title">SRAM Usage</div>
                <div class="card-value" id="ram-used">--</div>
                <div class="card-unit" id="ram-total">of 128 KB</div>
                <div class="progress-bar">
                    <div class="progress-fill" id="ram-progress" style="width: 0%"></div>
                </div>
            </div>

            <div class="card">
                <div class="card-title">CPU Load</div>
                <div class="card-value" id="cpu-load">--</div>
                <div class="card-unit">percent</div>
                <div class="progress-bar">
                    <div class="progress-fill" id="cpu-progress" style="width: 0%"></div>
                </div>
            </div>

            <div class="card">
                <div class="card-title">Free Memory</div>
                <div class="card-value" id="ram-free">--</div>
                <div class="card-unit">KB available</div>
            </div>

            <div class="card">
                <div class="card-title">Heartbeat</div>
                <div class="card-value status-online" id="heartbeat">●</div>
                <div class="card-unit" id="heartbeat-status">Active</div>
            </div>
        </div>

        <div class="footer">
            EspOS v0.1.0 | Powered by Embassy & Rust 🦀
        </div>
    </div>

    <script>
        async function updateStats() {
            try {
                const response = await fetch('/api/stats');
                const data = await response.json();

                document.getElementById('wifi-status').textContent = data.wifi_connected ? '● ONLINE' : '○ OFFLINE';
                document.getElementById('wifi-status').className = data.wifi_connected ? 'card-value status-online' : 'card-value status-offline';
                document.getElementById('wifi-ip').textContent = data.wifi_ip || 'No IP';
                document.getElementById('uptime').textContent = data.uptime_secs;

                const ramUsedKB = Math.round(data.ram_used / 1024);
                const ramFreeKB = Math.round(data.ram_free / 1024);
                const ramPercent = Math.round((data.ram_used / (data.ram_used + data.ram_free)) * 100);
                document.getElementById('ram-used').textContent = ramUsedKB;
                document.getElementById('ram-free').textContent = ramFreeKB;
                document.getElementById('ram-progress').style.width = ramPercent + '%';

                document.getElementById('cpu-load').textContent = data.cpu_percent;
                document.getElementById('cpu-progress').style.width = data.cpu_percent + '%';

                const heartbeatOk = data.heartbeat_hz > 0;
                document.getElementById('heartbeat').textContent = heartbeatOk ? '●' : '○';
                document.getElementById('heartbeat-status').textContent = heartbeatOk ? 'Active' : 'Stalled';

            } catch (error) {
                console.error('Failed to fetch stats:', error);
            }
        }

        updateStats();
        setInterval(updateStats, 1000);
    </script>
</body>
</html>"#;

/// Embassy task running the HTTP server on port 80
#[task]
pub async fn web_server_task(stack: &'static Stack<'static>) {
    log::info!("[web_server] task started");

    unsafe { BOOT_TIME = Some(Instant::now()); }

    stack.wait_config_up().await;
    log::info!("[web_server] HTTP server listening on port 80");

    let mut rx_buffer = [0; 2048];
    let mut tx_buffer = [0; 4096];

    loop {
        let mut socket = TcpSocket::new(*stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        if let Err(e) = socket.accept(80).await {
            log::warn!("[web_server] accept error: {:?}", e);
            continue;
        }

        let mut buf = [0u8; 512];
        match socket.read(&mut buf).await {
            Ok(n) if n > 0 => {
                let request = core::str::from_utf8(&buf[..n]).unwrap_or("");

                if request.starts_with("GET /api/stats") {
                    handle_api_stats(&mut socket).await;
                } else {
                    handle_dashboard(&mut socket).await;
                }
            }
            _ => {}
        }

        socket.close();
    }
}

async fn handle_dashboard(socket: &mut TcpSocket<'_>) {
    let _ = socket.write_all(HTTP_HEADER.as_bytes()).await;
    let _ = socket.write_all(HTML_DASHBOARD.as_bytes()).await;
    socket.flush().await.ok();
}

async fn handle_api_stats(socket: &mut TcpSocket<'_>) {
    let ram_used = memory::heap_used();
    let ram_free = memory::heap_free();
    let cpu_percent = LAST_CPU_PERCENT.load(core::sync::atomic::Ordering::Relaxed);
    let heartbeat_hz = HEARTBEAT_TICKS.load(core::sync::atomic::Ordering::Relaxed);

    let uptime_secs = unsafe {
        BOOT_TIME.map(|boot| Instant::now().duration_since(boot).as_secs()).unwrap_or(0)
    };

    let mut json: heapless::String<512> = heapless::String::new();
    let _ = core::fmt::write(
        &mut json,
        format_args!(
            r#"{{"wifi_connected":true,"wifi_ip":"<connected>","uptime_secs":{},"ram_used":{},"ram_free":{},"cpu_percent":{},"heartbeat_hz":{}}}"#,
            uptime_secs, ram_used, ram_free, cpu_percent, heartbeat_hz
        ),
    );

    let _ = socket.write_all(HTTP_JSON_HEADER.as_bytes()).await;
    let _ = socket.write_all(json.as_bytes()).await;
    socket.flush().await.ok();
}
