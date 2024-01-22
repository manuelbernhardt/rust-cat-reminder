use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::sleep;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Europe::Vienna;
use chrono::Timelike;

use gpiod::{Chip, Options};
use rs_ws281x::RawColor;
use crate::led::{LedController, RPILedController};
use crate::network::NetworkEvent;


const BLINK_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
const LOOP_DELAY: std::time::Duration = std::time::Duration::from_millis(1000);
const GPIO_BUTTON_PIN: u32 = 5;

pub enum ReminderModuleEvent {
    CleaningTimeUpdate(DateTime<Utc>)
}

#[derive(PartialEq)]
enum LEDStripState {
    LightGreen,
    DarkGreen,
    Orange,
    Red,
    BlinkingRed
}

impl LEDStripState {
    fn state_from_duration(duration: &Duration) -> Self {
        match duration.num_seconds() {
            0..=7 => LEDStripState::LightGreen,
            8..=11 => LEDStripState::DarkGreen,
            12..=23 => LEDStripState::Orange,
            24..=25 => LEDStripState::Red,
            _ => LEDStripState::BlinkingRed
        }
    }

    fn controller_color(&self) -> RawColor {
        match self {
            LEDStripState::LightGreen => RPILedController::LIGHT_GREEN,
            LEDStripState::DarkGreen => RPILedController::DARK_GREEN,
            LEDStripState::Orange => RPILedController::ORANGE,
            LEDStripState::Red => RPILedController::RED,
            LEDStripState::BlinkingRed => RPILedController::RED
        }
    }
}
pub struct Reminder {
    pub chip: Chip,
    pub controller: RPILedController,
    pub reminder_rx: Receiver<ReminderModuleEvent>,
    pub network_tx: Sender<NetworkEvent>,
    pub last_cleaning_time: DateTime<Utc>,
    pub is_strip_on: bool
}

impl Reminder {
    pub fn run(&mut self, shutdown_hook: Arc<AtomicBool>) {

        while !shutdown_hook.load(Ordering::Relaxed) {
            self.reset_state_if_button_pushed();

            if let Ok(event) = self.reminder_rx.try_recv() {
                match event {
                    ReminderModuleEvent::CleaningTimeUpdate(updated_cleaning_time) => {
                        log::info!("New cleaning time from network");
                        self.last_cleaning_time = updated_cleaning_time;
                    }
                }
            }

            let now = Utc::now().with_timezone(&Vienna);
            let is_night = now.hour() >= 22 || now.hour() < 7;
            let time_elapsed = Utc::now().signed_duration_since(self.last_cleaning_time);
            let current_state = LEDStripState::state_from_duration(&time_elapsed);

            if is_night && self.is_strip_on {
                // go dark
                self.controller.set_all_to(RPILedController::BLACK);
                self.is_strip_on = false;
            } else if !is_night {
                if current_state == LEDStripState::BlinkingRed {
                    if self.is_strip_on {
                        self.controller.set_all_to(RPILedController::BLACK);
                        self.is_strip_on = false;
                    } else {
                        self.controller.set_all_to(RPILedController::RED);
                        self.is_strip_on = true;
                    }
                } else {
                    self.controller.set_all_to(LEDStripState::controller_color(&current_state));
                }
            }

            if current_state == LEDStripState::BlinkingRed {
                sleep(BLINK_DELAY);
            } else {
                sleep(LOOP_DELAY);
            }
        }

        self.controller.set_all_to(RPILedController::BLACK);
    }

    /// Checks if the button was pushed and if so, resets the state
    fn reset_state_if_button_pushed(&mut self) {
        let button_pushed = self.read_button_state().unwrap();
        if button_pushed {
            // reset
            self.last_cleaning_time = crate::reset_state();
            self.network_tx.send(NetworkEvent::StateUpdated(self.last_cleaning_time)).expect("Could not send updated state");
        }
    }

    /// Reads the push button state. Expects the button to be connected at [GPIO_BUTTON_PIN]
    ///
    /// # Errors
    ///
    /// This function will return an error if the GPIO value cannot be read.
    fn read_button_state(&self) -> std::io::Result<bool> {
        let opts = Options::input([GPIO_BUTTON_PIN]);
        let inputs = self.chip.request_lines(opts)?;
        let values = inputs.get_values([false; 1])?;
        // false if pushed
        Ok(!values[0])
    }
}