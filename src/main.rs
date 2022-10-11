use std::{thread, time};
use std::fs::File;
use std::fs;
use std::path::Path;

use std::io::{BufRead, BufReader, Error, ErrorKind, Read};
use std::io::ErrorKind::InvalidData;
use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use tokio_cron_scheduler::{JobScheduler, JobToRun, Job};
use smart_leds::{SmartLedsWrite, RGB8};
use ws281x_rpi::Ws2812Rpi;

const NUM_LEDS: usize = 10;

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
async fn main() {
    println!("Program start");

    const PIN: i32 = 18;
    const DELAY: time::Duration = time::Duration::from_millis(1000);

    const DELAY_DARK_GREEN: time::Duration = time::Duration::from_secs(8 * 60 * 60);
    const DELAY_ORANGE: time::Duration = time::Duration::from_secs(12 * 60 * 60);
    const DELAY_RED: time::Duration = time::Duration::from_secs(24 * 60 * 60);
    const DELAY_DARK_RED: time::Duration = time::Duration::from_secs(28 * 60 * 60);
    const DELAY_RAINBOW: time::Duration = time::Duration::from_secs(30 * 60 * 60);

    let mut sched = JobScheduler::new();
    let mut ws = Ws2812Rpi::new(NUM_LEDS as i32, PIN).unwrap();

    let mut data: [RGB8; NUM_LEDS] = [RGB8::default(); NUM_LEDS];
    let empty: [RGB8; NUM_LEDS] = [RGB8::default(); NUM_LEDS];

    let mut last_cleaning_time: DateTime<Utc> = load_state();

    sched.add(Job::new("1/10 * * * * *", |uuid, l| {
        println!("I run every 10 seconds");
        let time_elapsed = Utc::now().signed_duration_since(last_cleaning_time);
        if time_elapsed < DELAY_DARK_GREEN {
            set_all_to(&mut ws, |led| led.g = 22 ); // TODO light green
        } else if time_elapsed < DELAY_ORANGE {
            set_all_to(&mut ws, |led| led.g = 32 ); // dark green
        } else if time_elapsed < DELAY_RED {
            set_all_to(&mut ws, |led| { // TODO orange
                led.g = 32;
                led.r = 32;
            })
        } else if time_elapsed < DELAY_DARK_RED { // TODO red blinking
            led.g = 0;
            led.r = 32;
        } else {
            // TODO rainbow
            led.g = 12;
            led.r = 32;
        }

    }).await.unwrap());


    // Blink the LED's in a blue-green-red-white pattern.
    for led in data.iter_mut().step_by(5) {
        led.b = 40; // blue
    }

    if NUM_LEDS > 1 {
        for led in data.iter_mut().skip(1).step_by(5) {
            led.g = 32; // green
        }
    }

    if NUM_LEDS > 2 {
        for led in data.iter_mut().skip(2).step_by(5) {
            led.r = 32; // red
        }
    }

    if NUM_LEDS > 3 {
        for led in data.iter_mut().skip(3).step_by(5) {
            // white
            led.r = 32;
            led.g = 32;
            led.b = 32;
        }
    }

    loop {
        // On
        println!("LEDS on");
        //ws.write(data.iter().cloned()).unwrap();
        set_all_to(&mut ws, |led| led.g = 32 );
        thread::sleep(DELAY);

        // Off
        println!("LEDS off");
        ws.write(empty.iter().cloned()).unwrap();
        thread::sleep(DELAY);
    }
}

fn set_all_to<C>(ws: &mut Ws2812Rpi, colorizer: C) -> () where C: Fn(&mut RGB8) -> () {
    let mut data: [RGB8; NUM_LEDS] = [RGB8::default(); NUM_LEDS];
    for led in data.iter_mut().step_by(1) {
        colorizer(led)
    }
    ws.write(data.iter().cloned()).unwrap();
}

fn load_state<R: Read>() -> DateTime<Utc> {
    return if Path::new(STATE_FILE_PATH).exists() {
        let time_str = fs::read_to_string(STATE_FILE_PATH);

        let parsed_time = time_str
            .and_then(|str| DateTime::parse_from_rfc3339(&*str).map_err(|e| Error::new(InvalidData, e)))
            .map(|t| t.with_timezone(&Utc));

        let time = match parsed_time {
            Ok(t) => t,
            Err(err) => {
                println!("Error reading time from state: {:?}", err);
                Utc::now();
            }
        };

        time
    } else {
        let now = Utc::now();
        fs::write(STATE_FILE_PATH, now.to_rfc3339()).unwrap();
        now
    }
}

