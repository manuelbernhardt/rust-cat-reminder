use std::fs;
use std::path::Path;
use std::io::Error;
use std::io::ErrorKind::InvalidData;
use std::sync::{Arc, mpsc};
use std::sync::atomic::AtomicBool;

use chrono::{DateTime, Utc};
use gpiod::{Chip};

use led::RPILedController;
use reminder::Reminder;

mod led;
mod network;
mod protocol;
mod mdns;
mod reminder;

const STATE_FILE_PATH: &str = "cat_reminder_state";

/// The Cat Litter Reminder, an annoying Raspberry PI with a LED Strip that signals when the cat litter box should be cleaned.
///
/// Main features:
/// - LEDs have different colors depending on how urgent it is to clean the litter box
/// - start to be really annoying when a full day has passed (blink in red)
/// - don't display any lights during the night
fn main() {
    env_logger::init();

    let chip: Chip = Chip::new("gpiochip0").expect("Cannot open GPIO");
    let controller = RPILedController::new();
    let last_cleaning_time: DateTime<Utc> = load_state();

    let ip_addr = local_ip_address::local_ip().expect("Could not resolve local IP address");

    let (reminder_tx, reminder_rx) = mpsc::channel();
    let (network_tx, network_rx) = mpsc::channel();

    let shutdown_hook = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&shutdown_hook)).unwrap();
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&shutdown_hook)).unwrap();
    signal_hook::flag::register(signal_hook::consts::SIGQUIT, Arc::clone(&shutdown_hook)).unwrap();

    mdns::run(ip_addr, 5200, network_tx.clone(), Arc::clone(&shutdown_hook));
    network::run(ip_addr, 5300, reminder_tx, network_rx, last_cleaning_time);

    let mut reminder = Reminder { chip, controller, reminder_rx, network_tx, last_cleaning_time, is_strip_on: false };
    reminder.run(Arc::clone(&shutdown_hook));
}




/// Loads the cat litter state (i.e. the last time at which the cat litter has been cleaned) from a file.
fn load_state() -> DateTime<Utc> {
    if Path::new(STATE_FILE_PATH).exists() {
        let time_str = fs::read_to_string(STATE_FILE_PATH);

        let parsed_time = time_str
            .and_then(|str| DateTime::parse_from_rfc3339(&*str).map_err(|e| Error::new(InvalidData, e)))
            .map(|t| t.with_timezone(&Utc));

        parsed_time.unwrap_or_else(|err| {
            log::error!("Error reading time from state: {:?}", err);
            Utc::now().to_owned()
        })
    } else {
        reset_state()
    }
}

/// Resets the state, i.e. sets the time at which the cat litter has been cleaned to now.
pub fn reset_state() -> DateTime<Utc> {
    let now = Utc::now();
    fs::write(STATE_FILE_PATH, now.to_rfc3339()).unwrap();
    now
}

