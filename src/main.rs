use std::time;
use std::fs;
use std::path::Path;
use std::io::Error;
use std::io::ErrorKind::InvalidData;

use actix::{Actor, Context};
use actix::prelude::*;
use chrono::{DateTime, Utc, Duration, Timelike};
use chrono_tz::Europe::Vienna;
use rs_ws281x::*;
use gpiod::{Chip, Options};

const BLINK_DELAY: time::Duration = time::Duration::from_millis(500);

const GPIO_BUTTON_PIN: u32 = 5;

const STATE_FILE_PATH: &str = "cat_reminder_state";


/*
    Program logic:
    - on start, retrieve state from file
    - state contains the time of the last cleaning
    - if no state, assume last reset now and write state to disk
    - periodically (5 min) check where we're at
      - now - state < DELAY_DARK_GREEN => set to light green
      - now - state > DELAY_DARK_GREEN => set to dark green
      - now - state > DELAY_ORANGE => set to orange
      - now - state > DELAY_RED => set to red
      - now - state > DELAY_RED => set to red and blink
      - now - state > DELAY_RED => set to multiple colors blinking in many different ways
    - when reset button pressed longer than 5 seconds
      - set state to now
      - store state
      - stop blinking, set to light green
 */
fn main() {
    println!("Program start");

    const NUM_LEDS: i32 = 10;
    const LED_PIN: i32 = 18;

    let system = actix::System::new();

    let last_cleaning_time: DateTime<Utc> = load_state();

    let chip: Chip = Chip::new("gpiochip0").expect("Cannot open GPIO");

    let controller: Controller = ControllerBuilder::new()
        .freq(800_000)
        .dma(10)
        .channel(
            0, // Channel Index
            ChannelBuilder::new()
                .pin(LED_PIN) // GPIO 10 = SPI0 MOSI
                .count(NUM_LEDS) // Number of LEDs
                .strip_type(StripType::Ws2812)
                .brightness(100) // default: 255
                .build(),
        )
        .build()
        .unwrap();


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

fn read_gpio(chip: &Chip) -> std::io::Result<bool> {
    let opts = Options::input([GPIO_BUTTON_PIN]);
    let inputs = chip.request_lines(opts)?;
    let values = inputs.get_values([false; 1])?;
    // false if pushed
    Ok(!values[0])
}


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

fn reset_state() -> DateTime<Utc> {
    let now = Utc::now();
    fs::write(STATE_FILE_PATH, now.to_rfc3339()).unwrap();
    now
}

#[derive(Message)]
#[rtype(result = "()")]
struct Tick;

#[derive(Message)]
#[rtype(result = "()")]
struct BlinkRed {
    led_on: bool
}

struct LedManager {
    chip: Chip,
    controller: Controller,
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

        let button_pushed = read_gpio(& self.chip).unwrap();
        if button_pushed {
            log::debug!("Button pushed");
            // reset
            self.last_cleaning_time = reset_state();

            if self.is_blinking {
                self.is_blinking = false;
            }
        }

        if is_night {
            if self.is_blinking {
                self.is_blinking = false;
            }
            set_all_to(&mut self.controller, [0, 0, 0, 0]);
        } else {
            let time_elapsed = Utc::now().signed_duration_since(self.last_cleaning_time);
            if time_elapsed < delay_dark_green {
                log::debug!("Light green");
                set_all_to(&mut self.controller, [0, 60, 0, 0]); // light green
            } else if time_elapsed < delay_orange {
                log::debug!("Dark green");
                set_all_to(&mut self.controller, [0, 20, 0, 0]); // dark green
            } else if time_elapsed < delay_red {
                log::debug!("Orange");
                set_all_to(&mut self.controller, [0, 60, 255, 0])
            } else if time_elapsed < delay_dark_red {
                log::debug!("Red");
                set_all_to(&mut self.controller, [0, 0, 255, 0]);
            } else {
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
            set_all_to(&mut self.controller, [0, 0, 0, 0]);
        } else if msg.led_on {
            // turn off
            set_all_to(&mut self.controller, [0, 0, 0, 0]);

            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                ctx.address().do_send(BlinkRed { led_on: false });
            });
        } else {
            // turn on
            set_all_to(&mut self.controller, [0, 0, 255, 0]);
            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                ctx.address().do_send(BlinkRed { led_on: true });
            });
        }
    }
}


fn set_all_to(controller: &mut Controller, color: RawColor) -> () {
    let leds = controller.leds_mut(0);
    for led in leds {
        *led = color
    }
    controller.render().unwrap();
}
