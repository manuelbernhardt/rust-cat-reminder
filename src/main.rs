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

    let system = actix::System::new();

    let chip: Chip = Chip::new("gpiochip0").expect("Cannot open GPIO");

    let controller = RPILedController::new();

    system.block_on(async {
        let _ = LedManager::create(|ctx| {
            let led_manager = ctx.address();

            let state = load_state();

            let (handler, listener) = node::split::<()>();

            let cluster_manager = ClusterManager::create(|_ctx| 
                ClusterManager { handler, listener, led_manager, last_cleaning_time: state
            });

            // before we start the led manager, check if we have a workable state, if not reset it
            let last_cleaning_time = match state {
                Some(state) => state,
                None => reset_state()
            };

            LedManager {
                chip,
                controller,
                last_cleaning_time,
                is_blinking: false,
                cluster_manager
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
fn load_state() -> Option<DateTime<Utc>> {
    if Path::new(STATE_FILE_PATH).exists() {
        let time_str = fs::read_to_string(STATE_FILE_PATH);

        let parsed_time = time_str
            .and_then(|str| DateTime::parse_from_rfc3339(&*str).map_err(|e| Error::new(InvalidData, e)))
            .map(|t| t.with_timezone(&Utc));

        match parsed_time {
            Ok(t) => Some(t),
            Err(err) => {
                log::error!("Error reading time from state: {:?}", err);
                None
            }
        }
    } else {
        None
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
    is_blinking: bool,
    cluster_manager: Addr<ClusterManager>
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


use message_io::network::{NetEvent, Transport};
use message_io::node::NodeHandler;
use message_io::node::NodeListener;
use message_io::node::{self};

struct ClusterManager {
    handler: NodeHandler<()>,
    listener: NodeListener<()>,
    led_manager: Addr<LedManager>,
    last_cleaning_time: Option<DateTime<Utc>>
}

impl Actor for ClusterManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {

        fn to_bytes(msg: CatMessage) -> [u8; 5] {
            let data = bincode::serialize(&msg).unwrap();
            data.try_into().unwrap()
        }

        let multicast_addr = "224.0.0.69:6666";
        let (endpoint, _) = self.handler.network().connect(Transport::Udp, multicast_addr).unwrap();

        // FIXME unfortunately we cannot use this with the actor model in rust.

        listener.for_each(move |event| match event.network() {
            NetEvent::Connected(_, _always_true_for_udp) => {
                log::info!("Connected to the network");
                match self.last_cleaning_time {
                    Some(_) => {}, // we're fine
                    None => {
                        log::info!("Asking other nodes for their state");
                        let msg = CatMessage { message_type: 0, timestamp: None };
                        self.handler.network().send(endpoint, &to_bytes(msg));

                        // TODO if we're the first node, we will never get a reply
                    }
                }
    
                self.handler.network().listen(Transport::Udp, multicast_addr).unwrap();
            }
            NetEvent::Message(_, data) => {

                let data = Vec::from(data);
                let msg = bincode::deserialize::<CatMessage>(&data);
                match msg {
                    Ok(cat_message) => {
                       match cat_message.message_type {
                            0 => {
                                // there's a new kid on the block
                                self.handler.network().send(endpoint, &to_bytes(CatMessage { message_type: 1, timestamp: self.last_cleaning_time }));
                            },
                            1 => {
                                // someone is broadcasting their state, let's check if it is interesting
                                // TODO if this state is more recent than ours, inform the led_manager about it
                                // TODO when the led_manager updates the state, inform this actor
                            }
                        }
                    },
                    Err(err) => {
                        log::error!("Could not parse message {}", err);
                    }
                }
            },
            NetEvent::Accepted(_, _) => unreachable!(), // UDP is not connection-oriented
            NetEvent::Disconnected(_) => ()
        });

    }

}

use serde::{Serialize, Deserialize};
use chrono::serde::ts_seconds_option;

//  0                   1                   2                   3
//  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |      Type     |                                               |
//  +-+-+-+-+-+-+-+-+                                               +
//  |                           Timestamp                           |
//  +               +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//  |               |                    Padding                    |
//  +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//
// message_type:
// - 0: i don't know anything
// - 1: here is my state
#[derive(Serialize, Deserialize, Debug)]
struct CatMessage {
    message_type: u8,
    #[serde(with = "ts_seconds_option")]
    timestamp: Option<DateTime<Utc>>
}