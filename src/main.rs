use std::time;
use std::fs;
use std::fs::read;
use std::path::Path;
use std::io::Error;
use std::io::ErrorKind::InvalidData;

use actix::{Actor, Context};
use actix::prelude::*;
use chrono::{DateTime, Utc, Duration};
use smart_leds::{SmartLedsWrite, RGB8};
use ws281x_rpi::Ws2812Rpi;
use gpiod::{Chip, Options, Masked, AsValuesMut};

const NUM_LEDS: usize = 10;

const BLINK_DELAY: time::Duration = time::Duration::from_millis(500);

const GPIO_BUTTON_PIN: u8 = 5;

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

    const LED_PIN: i32 = 18;

    let system = actix::System::new();

    let last_cleaning_time: DateTime<Utc> = load_state();

    read_gpio();

    system.block_on(async {
        let _ = LedManager::create(|_ctx| {
            LedManager {
                ws: Ws2812Rpi::new(NUM_LEDS as i32, LED_PIN).unwrap(),
                last_cleaning_time,
                is_blinking: false
            }
        });
    });

    system.run().unwrap();
}

fn read_gpio() -> std::io::Result<()> {
    let chip = Chip::new("gpiochip0")?; // open chip

    let opts = Options::input([5]) // configure lines offsets
    .consumer("my-inputs"); // optionally set consumer string

    let inputs = chip.request_lines(opts)?;

    // get all three values
    let values = inputs.get_values([false; 1])?;

    println!("values: {:?}", values);

    Ok(())
}


fn load_state() -> DateTime<Utc> {
    return if Path::new(STATE_FILE_PATH).exists() {
        let time_str = fs::read_to_string(STATE_FILE_PATH);

        let parsed_time = time_str
            .and_then(|str| DateTime::parse_from_rfc3339(&*str).map_err(|e| Error::new(InvalidData, e)))
            .map(|t| t.with_timezone(&Utc));

        let time = match parsed_time {
            Ok(t) => t,
            Err(err) => {
                println!("Error reading time from state: {:?}", err);
                Utc::now().to_owned()
            }
        };

        time
    } else {
        let now = Utc::now();
        fs::write(STATE_FILE_PATH, now.to_rfc3339()).unwrap();
        now
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

struct LedManager {
    ws: Ws2812Rpi,
    last_cleaning_time: DateTime<Utc>,
    is_blinking: bool
}

impl Actor for LedManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // first tick for initialization right away
        ctx.address().do_send(Tick { });

        // schedule check every 10 seconds
        let _ = ctx.run_interval(time::Duration::from_secs(1), |_this, ctx| {
            println!("Ticking");
            ctx.address().do_send(Tick { });
        });
    }

}

impl Handler<Tick> for LedManager {
    type Result = ();


    fn handle(&mut self, _msg: Tick, ctx: &mut Self::Context) -> Self::Result {
        println!("Tick received");

        let delay_dark_green: Duration = Duration::seconds(8);
        let delay_orange: Duration = Duration::seconds(12);
        let delay_red: Duration = Duration::seconds(24);
        let delay_dark_red: Duration = Duration::seconds(26);


        let time_elapsed = Utc::now().signed_duration_since(self.last_cleaning_time);
        if time_elapsed < delay_dark_green {
            println!("Light green");
            set_all_to(&mut self.ws, |led| {
                led.r = 50;
                led.g = 174;
                led.b = 0;
            });
        } else if time_elapsed < delay_orange {
            println!("Dark green");
            set_all_to(&mut self.ws, |led| {
                led.r = 0;
                led.g = 60;
                led.b = 0;
            }); // dark green
        } else if time_elapsed < delay_red {
            println!("Orange");
            set_all_to(&mut self.ws, |led| {
                led.r = 200;
                led.g = 165;
                led.b = 0;
            })
        } else if time_elapsed < delay_dark_red {
            println!("Red");
            set_all_to(&mut self.ws, |led| {
                led.r = 255;
                led.g = 0;
                led.b = 0;
            });
        } else {
            if !self.is_blinking {
                println!("Blinking red");
                self.is_blinking = true;
                ctx.address().do_send(BlinkRed { led_on: false});
            }
        }


    }
}

impl Handler<BlinkRed> for LedManager {
    type Result = ();

    fn handle(&mut self, msg: BlinkRed, ctx: &mut Self::Context) -> Self::Result {
        println!("Blinking red received");
        if msg.led_on {
            // turn off
            let empty: [RGB8; NUM_LEDS] = [RGB8::default(); NUM_LEDS];
            self.ws.write(empty.iter().cloned()).unwrap();

            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                println!("Blinking red off");
                ctx.address().do_send(BlinkRed { led_on: false });
            });
        } else {
            // turn on
            set_all_to(&mut self.ws, |led| {
                led.r = 255;
                led.g = 0;
                led.b = 0;
            });
            let _ = ctx.run_later(BLINK_DELAY, |_this, ctx| {
                println!("Blinking red on");
                ctx.address().do_send(BlinkRed { led_on: true });
            });
        }
    }
}

fn set_all_to<C>(ws: &mut Ws2812Rpi, colorizer: C) -> () where C: Fn(&mut RGB8) -> () {
    let mut data: [RGB8; NUM_LEDS] = [RGB8::default(); NUM_LEDS];
    for led in data.iter_mut().step_by(1) {
        colorizer(led)
    }
    ws.write(data.iter().cloned()).unwrap();
}
