use std::time;
use std::fs;
use std::path::Path;
use std::io::Error;
use std::io::ErrorKind::InvalidData;

use actix::{Actor, Context};
use actix::prelude::*;
use chrono::{DateTime, Utc, Duration, Timelike};
use chrono_tz::Europe::Vienna;
use gpiod::{Chip, Options};

mod led;
use led::{LedController, RPILedController};

const BLINK_DELAY: time::Duration = time::Duration::from_millis(500);

const GPIO_BUTTON_PIN: u32 = 5;

const STATE_FILE_PATH: &str = "cat_reminder_state";

/// The Cat Litter Reminder, an annoying Raspberry PI with a LED Strip that signals when the cat litter box should be cleaned.
/// 
/// Main features:
/// - LEDs have different colors depending on how urgent it is to clean the litter box
/// - start to be really annoying when a full day has passed (blink in red)
/// - don't display any lights during the night
fn main() {

    let last_cleaning_time: DateTime<Utc> = load_state();

    let system = actix::System::new();

    let chip: Chip = Chip::new("gpiochip0").expect("Cannot open GPIO");

    let controller = RPILedController::new();


    system.block_on(async {
        let _ = LedManager::create(|_ctx| {
            LedManager {
                chip,
                controller,
                last_cleaning_time,
                is_blinking: false
            }
        });
    });

    system.run().unwrap();
}

/// Reads the push button state. Expects the button to be connected at [GPIO_BUTTON_PIN]
///
/// # Errors
///
/// This function will return an error if the GPIO value cannot be read.
fn read_button_state(chip: &Chip) -> std::io::Result<bool> {
    let opts = Options::input([GPIO_BUTTON_PIN]);
    let inputs = chip.request_lines(opts)?;
    let values = inputs.get_values([false; 1])?;
    // false if pushed
    Ok(!values[0])
}

/// Loads the cat litter state (i.e. the last time at which the cat litter has been cleaned) from a file.
fn load_state() -> DateTime<Utc> {
    if Path::new(STATE_FILE_PATH).exists() {
        let time_str = fs::read_to_string(STATE_FILE_PATH);

        let parsed_time = time_str
            .and_then(|str| DateTime::parse_from_rfc3339(&*str).map_err(|e| Error::new(InvalidData, e)))
            .map(|t| t.with_timezone(&Utc));

        match parsed_time {
            Ok(t) => t,
            Err(err) => {
                log::error!("Error reading time from state: {:?}", err);
                Utc::now().to_owned()
            }
        }
    } else {
        reset_state()
    }
}

/// Resets the state, i.e. sets the time at which the cat litter has been cleaned to now.
fn reset_state() -> DateTime<Utc> {
    let now = Utc::now();
    fs::write(STATE_FILE_PATH, now.to_rfc3339()).unwrap();
    now
}

struct LedManager {
    chip: Chip,
    controller: RPILedController,
    last_cleaning_time: DateTime<Utc>,
    is_blinking: bool
}

impl Actor for LedManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // first tick for initialization right away
        ctx.address().do_send(Tick { });

        // schedule check every 5 seconds
        let _ = ctx.run_interval(time::Duration::from_secs(5), |_this, ctx| {
            ctx.address().do_send(Tick { });
        });
    }

}

impl Handler<Tick> for LedManager {
    type Result = ();

    fn handle(&mut self, _msg: Tick, ctx: &mut Self::Context) -> Self::Result {
        log::debug!("Tick received");

        let now = Utc::now().with_timezone(&Vienna);
        let is_night = now.hour() >= 22 || now.hour() < 7;

        let delay_dark_green: Duration = Duration::hours(8);
        let delay_orange: Duration = Duration::hours(12);
        let delay_red: Duration = Duration::hours(24);
        let delay_dark_red: Duration = Duration::hours(26);

        let button_pushed = read_button_state(& self.chip).unwrap();
        if button_pushed {
            log::debug!("Button pushed");
            // reset
            self.last_cleaning_time = reset_state();

            if self.is_blinking {
                self.is_blinking = false;
            }
        }

        if is_night {
            // don't blink in red at night, it's annoying
            if self.is_blinking {
                self.is_blinking = false;
            }
            // go dark
            self.controller.set_all_to(RPILedController::BLACK);
        } else {
            let time_elapsed = Utc::now().signed_duration_since(self.last_cleaning_time);
            if time_elapsed < delay_dark_green {
                log::debug!("Light green");
                self.controller.set_all_to(RPILedController::LIGHT_GREEN);
            } else if time_elapsed < delay_orange {
                log::debug!("Dark green");
                self.controller.set_all_to(RPILedController::DARK_GREEN);
            } else if time_elapsed < delay_red {
                log::debug!("Orange");
                self.controller.set_all_to(RPILedController::ORANGE);
            } else if time_elapsed < delay_dark_red {
                log::debug!("Red");
            self.controller.set_all_to(RPILedController::RED);
            } else {
                log::debug!("Blinking red");
                if !self.is_blinking {
                    self.is_blinking = true;
                    ctx.address().do_send(BlinkRed { led_on: false});
                }
            }
        }

    }
}

impl Handler<BlinkRed> for LedManager {
    type Result = ();

    fn handle(&mut self, msg: BlinkRed, ctx: &mut Self::Context) -> Self::Result {
        if !self.is_blinking {
            // turn off
            self.controller.set_all_to(RPILedController::BLACK);
            return;
        }
        
        if msg.led_on {
            // turn off
            self.controller.set_all_to(RPILedController::BLACK);

            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                ctx.address().do_send(BlinkRed { led_on: false });
            });
        } else {
            // turn on
            self.controller.set_all_to(RPILedController::RED);
            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                ctx.address().do_send(BlinkRed { led_on: true });
            });
        }
    }
}

#[derive(Message)]
#[rtype(result = "()")]
struct Tick;

#[derive(Message)]
#[rtype(result = "()")]
struct BlinkRed {
    led_on: bool
}
