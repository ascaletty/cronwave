use chrono::TimeZone;

use chrono::{DateTime, Duration, Local, NaiveDateTime, Utc};
use icalendar::DatePerhapsTime;
use rrule::{RRule, Unvalidated, Validated};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct Task {
    pub id: usize,
    pub uuid: String,
    pub description: String,
    pub due: i64,
    pub estimated: i64,
    pub status: String,
    pub urgency: f32,
}

#[derive(Debug, Deserialize)]
pub struct RawTask {
    pub id: usize,
    pub uuid: String,
    pub description: String,
    pub due: String,
    pub estimated: String,
    pub status: String,
    pub urgency: f32,
}

#[derive(Debug)]
pub struct TimeBlockRaw {
    pub rrule: Option<RRule<Unvalidated>>,
    pub dtstart: DatePerhapsTime,
    pub duration: Option<String>,
    pub dtend: Option<String>,
    pub uid: String,
    pub summary: String,
    pub dtstamp: DateTime<Utc>,
}
#[derive(Debug, Clone)]
pub struct TimeBlock {
    pub rrule: Option<RRule<Validated>>,
    pub dtstart: i64,
    pub duration: Option<i64>,
    pub dtend: Option<i64>,
    pub uid: String,
    pub summary: String,
    pub dtstamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Gap {
    //we are going to store our start and ends as unix timestamps
    pub start: i64,
    pub end: i64,
}
impl Gap {
    pub fn last(last_block: i64, greatest_dur: i64) -> Self {
        Self {
            start: last_block,
            end: greatest_dur,
        }
    }
}
// impl TimeBlock {
//     pub fn last(last_block: i64, greatest_duration: i64) -> Self {
//         Self {
//             duration: Some(greatest_duration),
//             dtstart: last_block,
//             summary: "last".to_string(),
//             uid: "615d0917-2955-4f3e-ae21-ca0f72bdc48a".to_string(),
//             dtstamp: Utc::now(),
//             dtend: None,
//             rrule: None,
//             class: None,
//         }
//     }
// }

#[derive(Deserialize, Clone, Serialize, Debug)]
pub struct basic {
    pub cal_url: String,
    pub cal_username: String,
    pub cal_pass: String,
}
pub struct oath2 {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Deserialize, Clone, Serialize, Debug)]
pub struct ConfigInfo {
    pub basic: basic,
    pub main: main,
}

#[derive(Deserialize, Clone, Serialize, Debug)]
pub struct main {
    pub days_ahead: i64,
}
impl ::std::default::Default for ConfigInfo {
    fn default() -> Self {
        Self {
            basic: basic {
                cal_url: "your cal url".to_string(),
                cal_username: "your cal_username".to_string(),
                cal_pass: "your cal password".to_string(),
            },
            main: main { days_ahead: 365 },
        }
    }
}
