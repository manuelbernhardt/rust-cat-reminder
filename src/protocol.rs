use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use chrono::serde::ts_seconds_option;

#[derive(Serialize, Deserialize)]
pub enum Message {
    RequestState,
    UpdateState(#[serde(with = "ts_seconds_option")] Option<DateTime<Utc>>)
}