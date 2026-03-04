//! # Agentic State Machine
//!
//! Top-level orchestrator that ties together all hardware traits and async
//! tasks.  The state machine implements the following workflow:
//!
//! ```text
//!  Idle
//!   │  audio trigger / command
//!   ▼
//!  Listening  ──(audio buffer full)──►  Processing
//!                                            │
//!                              WiFi LLM API  │
//!                                            ▼
//!                                       Moving  ──(done)──►  Reporting
//!                                                                │
//!                                                      update UI │
//!                                                                ▼
//!                                                             Idle
//! ```
//!
//! Collision alerts from [`crate::tasks::can_bus::COLLISION_CHANNEL`] can
//! interrupt *any* state and transition immediately to an emergency stop.

extern crate alloc;

use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_time::Timer;

use crate::tasks::can_bus::COLLISION_CHANNEL;
use crate::tasks::imu::IMU_CHANNEL;
use crate::tasks::motor::{MotorCommand, MOTOR_CHANNEL};
use crate::tasks::ui::{UiDrawCommand, UI_DRAW_CHANNEL};
use crate::tasks::wifi::IP_CHANNEL;

// ---------------------------------------------------------------------------
// State enum
// ---------------------------------------------------------------------------

/// All possible states of the agentic rover.
#[derive(Debug, Clone, PartialEq)]
pub enum RoverState {
    /// System is idle; waiting for a trigger (voice, CAN command, or WiFi).
    Idle,

    /// Actively capturing audio samples into a ring buffer.
    Listening,

    /// Audio capture complete; waiting for the LLM API response over WiFi.
    Processing {
        /// Raw audio payload length captured during the Listening state.
        audio_bytes: usize,
    },

    /// Executing a motion command parsed from the LLM response.
    Moving {
        /// Human-readable description of the motion command.
        description: heapless::String<64>,
    },

    /// Motion complete; publishing telemetry and updating the UI.
    Reporting,

    /// Emergency stop triggered by a CAN collision alert.
    EmergencyStop,
}

// ---------------------------------------------------------------------------
// Event enum
// ---------------------------------------------------------------------------

/// Internal events that drive state transitions.
#[derive(Debug)]
enum Event {
    /// A CAN collision frame was received – highest priority.
    CollisionAlert,
    /// An IP address was obtained; WiFi is usable.
    WifiReady(heapless::String<16>),
    /// IMU detected significant motion that should be logged.
    ImuUpdate,
    /// 500 ms tick used for periodic checks in the Idle state.
    Tick,
}

// ---------------------------------------------------------------------------
// State machine entry point
// ---------------------------------------------------------------------------

/// Run the agentic state machine on the calling task (Core 0 main task).
///
/// This function never returns; it loops forever processing events and
/// transitioning between states.
pub async fn run(_spawner: &Spawner) {
    log::info!("[state_machine] starting");

    let mut state = RoverState::Idle;
    let mut ip_address: Option<heapless::String<16>> = None;

    // Publish the initial screen.
    push_ui(UiDrawCommand::ShowState({
        let mut s = heapless::String::new();
        let _ = core::fmt::write(&mut s, format_args!("Idle"));
        s
    }))
    .await;

    loop {
        // ---- Collision check (highest priority – checked every iteration) -
        if let Ok(frame) = COLLISION_CHANNEL.try_receive() {
            log::warn!(
                "[state_machine] COLLISION id=0x{:03X} – emergency stop",
                frame.id
            );
            handle_emergency_stop().await;
            state = RoverState::EmergencyStop;
        }

        // ---- State-specific logic ----------------------------------------
        match &state {
            // ----------------------------------------------------------------
            RoverState::Idle => {
                // Wait for either a WiFi IP or a 2-second idle tick.
                let event = select(
                    async { IP_CHANNEL.receive().await },
                    Timer::after_millis(2_000),
                )
                .await;

                match event {
                    Either::First(ip) => {
                        log::info!("[state_machine] WiFi ready: {}", ip.as_str());
                        ip_address = Some(ip.clone());
                        push_ui(UiDrawCommand::StatusText({
                            let mut s = heapless::String::new();
                            let _ = core::fmt::write(&mut s, format_args!("IP: {}", ip.as_str()));
                            s
                        }))
                        .await;
                        // Transition: start listening for a voice command.
                        state = RoverState::Listening;
                        update_state_ui("Listening").await;
                    }
                    Either::Second(_tick) => {
                        log::debug!("[state_machine] idle tick");
                    }
                }
            }

            // ----------------------------------------------------------------
            RoverState::Listening => {
                // Simulate capturing 1 second of audio at 16 kHz mono 16-bit
                // = 32 000 bytes.
                log::info!("[state_machine] capturing audio…");
                push_ui(UiDrawCommand::ProgressBar {
                    percent: 0,
                    label: heapless::String::try_from("Listening").unwrap_or_default(),
                })
                .await;

                // Simulate 1 second of capture with progress updates.
                for i in 1..=10u8 {
                    Timer::after_millis(100).await;
                    push_ui(UiDrawCommand::ProgressBar {
                        percent: i * 10,
                        label: heapless::String::try_from("Listening").unwrap_or_default(),
                    })
                    .await;
                }

                state = RoverState::Processing { audio_bytes: 32_000 };
                update_state_ui("Processing").await;
            }

            // ----------------------------------------------------------------
            RoverState::Processing { audio_bytes } => {
                let bytes = *audio_bytes;
                log::info!("[state_machine] sending {} bytes to LLM…", bytes);

                if ip_address.is_some() {
                    // In a real build: open a TCP socket, POST the audio to
                    // the LLM endpoint, parse the JSON response.
                    Timer::after_millis(800).await; // Simulate network RTT.

                    // Use a safe fallback command if the actual LLM response
                    // (substituted here for "forward 0.5") exceeds the 64-byte
                    // heapless buffer capacity in production paths.
                    let cmd_text: heapless::String<64> =
                        heapless::String::try_from("forward 0.5").unwrap_or_else(|_| {
                            log::warn!("[state_machine] parsed command exceeds buffer (64 chars) – defaulting to stop");
                            heapless::String::try_from("stop").unwrap_or_default()
                        });

                    log::info!("[state_machine] LLM response: '{}'", cmd_text.as_str());
                    state = RoverState::Moving {
                        description: cmd_text,
                    };
                    update_state_ui("Moving").await;
                } else {
                    log::warn!("[state_machine] no WiFi – skipping LLM, returning to Idle");
                    state = RoverState::Idle;
                    update_state_ui("Idle").await;
                }
            }

            // ----------------------------------------------------------------
            RoverState::Moving { description } => {
                let desc = description.clone();
                log::info!("[state_machine] executing: '{}'", desc.as_str());

                // Parse the command and dispatch to the motor task.
                parse_and_dispatch_motion(desc.as_str()).await;

                // Allow 2 seconds for the motion to complete.
                Timer::after_millis(2_000).await;

                MOTOR_CHANNEL.send(MotorCommand::Stop).await;

                state = RoverState::Reporting;
                update_state_ui("Reporting").await;
            }

            // ----------------------------------------------------------------
            RoverState::Reporting => {
                log::info!("[state_machine] reporting telemetry");

                // Drain the latest IMU reading for the report.
                if let Ok(imu) = IMU_CHANNEL.try_receive() {
                    log::info!(
                        "[state_machine] final attitude roll={:.1}° pitch={:.1}°",
                        imu.roll,
                        imu.pitch
                    );
                }

                push_ui(UiDrawCommand::StatusText(
                    heapless::String::try_from("Done").unwrap_or_default(),
                ))
                .await;

                // Return to Idle after a brief pause.
                Timer::after_millis(1_000).await;
                state = RoverState::Idle;
                update_state_ui("Idle").await;
            }

            // ----------------------------------------------------------------
            RoverState::EmergencyStop => {
                // Stay in emergency stop until the state is externally cleared
                // (e.g., a CAN "all-clear" message or a button press).
                Timer::after_millis(500).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a simple text command and send the appropriate [`MotorCommand`].
async fn parse_and_dispatch_motion(cmd: &str) {
    if cmd.starts_with("forward") {
        let speed = parse_speed(cmd).unwrap_or(0.5);
        MOTOR_CHANNEL.send(MotorCommand::Forward(speed)).await;
    } else if cmd.starts_with("backward") || cmd.starts_with("back") {
        let speed = parse_speed(cmd).unwrap_or(0.5);
        MOTOR_CHANNEL.send(MotorCommand::Backward(speed)).await;
    } else if cmd.starts_with("rotate") || cmd.starts_with("turn") {
        let angle = parse_angle(cmd).unwrap_or(90.0);
        MOTOR_CHANNEL.send(MotorCommand::Rotate(angle)).await;
    } else if cmd.contains("stop") || cmd.contains("brake") {
        MOTOR_CHANNEL.send(MotorCommand::Brake).await;
    } else {
        log::warn!("[state_machine] unknown motion command: '{}'", cmd);
    }
}

/// Apply emergency braking and update the UI.
async fn handle_emergency_stop() {
    MOTOR_CHANNEL.send(MotorCommand::Brake).await;
    use embedded_graphics::prelude::RgbColor;
    push_ui(UiDrawCommand::Clear(embedded_graphics::pixelcolor::Rgb565::RED)).await;
    push_ui(UiDrawCommand::StatusText(
        heapless::String::try_from("EMERGENCY STOP").unwrap_or_default(),
    ))
    .await;
}

/// Send a state-name string to the UI task.
async fn update_state_ui(name: &str) {
    let mut s: heapless::String<32> = heapless::String::new();
    let _ = core::fmt::write(&mut s, format_args!("{}", name));
    push_ui(UiDrawCommand::ShowState(s)).await;
}

/// Non-blocking push to the UI channel (drops if full).
async fn push_ui(cmd: UiDrawCommand) {
    let _ = UI_DRAW_CHANNEL.try_send(cmd);
}

// ---------------------------------------------------------------------------
// Tiny number parsers (no_std, no regex)
// ---------------------------------------------------------------------------

/// Extract a speed float from strings like `"forward 0.7"`.
fn parse_speed(s: &str) -> Option<f32> {
    s.split_whitespace().nth(1).and_then(|tok| parse_f32(tok))
}

/// Extract an angle float from strings like `"rotate 90"`.
fn parse_angle(s: &str) -> Option<f32> {
    s.split_whitespace().nth(1).and_then(|tok| parse_f32(tok))
}

/// Parse a decimal float string in no_std without `std::str::parse`.
fn parse_f32(s: &str) -> Option<f32> {
    let mut result = 0.0f32;
    let mut fraction = false;
    let mut divisor = 10.0f32;
    let mut negative = false;

    for (i, c) in s.chars().enumerate() {
        match c {
            '-' if i == 0 => negative = true,
            '.' => fraction = true,
            '0'..='9' => {
                let digit = (c as u8 - b'0') as f32;
                if fraction {
                    result += digit / divisor;
                    divisor *= 10.0;
                } else {
                    result = result * 10.0 + digit;
                }
            }
            _ => return None,
        }
    }

    if negative {
        result = -result;
    }
    Some(result)
}
