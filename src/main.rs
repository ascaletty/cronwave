use chrono::Utc;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use ical::IcalParser;
use iso8601::Duration as isoDuration;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::process::Command;
use std::str::FromStr;
use std::sync::Mutex;
use uuid::Uuid;

use crate::config::ConfigInfo;
mod config;
#[derive(Debug, Clone)]
pub struct Task {
    uuid: String,
    description: String,
    due: DateTime<Utc>,
    estimated: Duration,
    status: Option<String>,
    urgency: f32,
}

#[derive(Debug, Deserialize)]
pub struct RawTask {
    uuid: String,
    description: String,
    due: Option<String>,
    estimated: Option<String>,
    status: Option<String>,
    urgency: f32,
}

#[derive(Debug, Deserialize)]
pub struct TimeBlockRaw {
    dtstart: String,
    duration: String,
    uid: String,
    summary: String,
    dtstamp: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TimeBlock {
    dtstart: chrono::DateTime<Local>,
    duration: Duration,
    uid: String,
    summary: String,
    dtstamp: chrono::DateTime<Local>,
}

fn create_caldav_events(
    scheduled: Vec<TimeBlock>,
    not_scheduled: Vec<TimeBlock>,
    config_data: ConfigInfo,
    blocks: Mutex<Vec<TimeBlock>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut string_vec = vec!["BEGIN:VCALENDAR".to_string()];
    for event in scheduled {
        blocks.lock().unwrap().push(event);
    }
    for event in not_scheduled {
        blocks.lock().unwrap().push(event);
    }
    for event in blocks.into_inner().unwrap() {
        let ics = format!(
            "
VERSION:2.0\r
PRODID:-//MyApp//EN\r
BEGIN:VEVENT\r
UID:{uid}\r
DTSTAMP:{now}\r
DTSTART:{dtstart}\r
DURATION:{duration}\r
SUMMARY:{summary}\r
END:VEVENT\r",
            uid = event.uid,
            now = event.dtstamp.format("%Y%m%dT%H%M%SZ"),
            dtstart = event.dtstart.format("%Y%m%dT%H%M%SZ"),
            duration = event.duration,
            summary = event.summary,
        )
        .to_string();
        string_vec.push(ics);
    }
    string_vec.push("END:VCALENDAR\r".to_string());
    let combined = string_vec.join("\n");
    println!("{combined}");

    // Set up HTTP client and headers
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/calendar"));

    let client = Client::new();

    // Send the PUT request with basic auth
    let response = client
        .put(config_data.cal_url)
        .basic_auth(config_data.cal_username, Some(config_data.cal_pass))
        .headers(headers)
        .body(combined)
        .send()?;

    if response.status().is_success() {
        println!("✅ Events created ");

        Ok(())
    } else {
        Err(format!("❌ Failed to create event: {}", response.status()).into())
    }
}
fn duration_to_minutes(duration: &iso8601::Duration) -> i64 {
    match duration {
        iso8601::Duration::Weeks(weeks) => *weeks as i64 * 7 * 24 * 60,
        iso8601::Duration::YMDHMS {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: _,
        } => {
            (*year as i64 * 365 * 24 * 60) +     // assume 365 days/year
                (*month as i64 * 30 * 24 * 60) +     // assume 30 days/month
                (*day as i64 * 24 * 60) +
                (*hour as i64 * 60) +
                (*minute as i64) +
                (*second as i64 / 60)
        }
    }
}
fn convert_raw_blocks(blocks: Vec<TimeBlockRaw>) -> Vec<TimeBlock> {
    let mut serialized_blocks = vec![];
    for block in blocks {
        let data = TimeBlock {
            dtstart: ical_basic_to_dtlocal(&block.dtstart),
            duration: Duration::minutes(duration_to_minutes(
                &isoDuration::from_str(&block.duration).unwrap_or(isoDuration::Weeks(0)),
            )),
            uid: block.uid,
            summary: block.summary,
            dtstamp: ical_basic_to_dtlocal(&block.dtstamp),
        };
        serialized_blocks.push(data);
    }
    serialized_blocks
}
fn ical_basic_to_dtlocal(ical_dt: &str) -> DateTime<Local> {
    let s = ical_dt.trim();
    let is_utc = s.ends_with('Z');

    let core = if is_utc { &s[..s.len() - 1] } else { s };

    if core.len() != 15 || !core.contains('T') {
        panic!("Invalid iCalendar datetime: {s}");
    }

    let date = &core[0..8]; // YYYYMMDD
    let time = &core[9..15]; // HHMMSS

    let rfc = format!(
        "{}-{}-{}T{}:{}:{}",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4],
        &time[4..6]
    );

    if is_utc {
        DateTime::from_str(&format!("{rfc}Z")).expect("failed to get datetime")
    } else {
        Local
            .from_local_datetime(&NaiveDateTime::from_str(&rfc).unwrap())
            .unwrap()
    }
}
fn schedule(tasks: Mutex<Vec<Task>>, config_data: ConfigInfo, blocks: Mutex<Vec<TimeBlock>>) {
    let blocks_copy = {
        let guard = blocks.lock().unwrap();
        guard.clone()
    };

    let mut scheduled = vec![];
    let mut not_scheduled = vec![];

    for block in &blocks_copy {
        let current_time = Local::now();
        let time_til = block.dtstart - current_time;

        let tasks_copy = {
            let guard = tasks.lock().unwrap();
            guard.clone()
        };

        let mut remaining_tasks = vec![];

        for task in tasks_copy {
            if task.uuid == block.uid {
                if task.estimated <= time_til {
                    scheduled.push(TimeBlock {
                        duration: task.estimated,
                        dtstart: block.dtstart,
                        uid: Uuid::new_v4().to_string(),
                        dtstamp: Local::now(),
                        summary: task.description.clone(),
                    });
                } else {
                    remaining_tasks.push(task);
                }
            }
        }

        {
            let mut guard = tasks.lock().unwrap();
            *guard = remaining_tasks;
        }
    }

    let last_block_time = {
        let guard = blocks.lock().unwrap();
        let last = guard.last().unwrap();
        last.dtstamp + last.duration
    };

    let tasks_remaining = {
        let guard = tasks.lock().unwrap();
        guard.clone()
    };
    for task in tasks_remaining {
        not_scheduled.push(TimeBlock {
            duration: task.estimated,
            dtstart: last_block_time,
            uid: Uuid::new_v4().to_string(),
            dtstamp: Local::now(),
            summary: task.description,
        });
    }

    // Now call without holding any lock
    create_caldav_events(scheduled, not_scheduled, config_data, blocks)
        .expect("failed to create_caldav_events");
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Input: "20250806T195556Z" → Output: DateTime<Utc>
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
}
fn fetch_tasks() -> Vec<Task> {
    let task_command = Command::new("task")
        .args(["status:pending", "export"])
        .output()
        .expect("failed to run task export");

    let output_raw: Vec<RawTask> =
        serde_json::from_slice(&task_command.stdout).expect("invalid taskwarrior output");
    let mut output = vec![];
    for task in output_raw {
        let task_item = Task {
            uuid: task.uuid,
            description: task.description,
            due: parse_timestamp(task.due.expect("failed to get scheduled time").as_str())
                .expect("failed timestamp parse"),
            estimated: Duration::minutes(duration_to_minutes(
                &isoDuration::from_str(&task.estimated.unwrap())
                    .expect("failed to parse iso8601 from str"),
            )),
            status: task.status,
            urgency: task.urgency,
        };
        output.push(task_item);
    }

    output.sort_by(|a, b| a.urgency.partial_cmp(&b.urgency).unwrap());
    output
}
fn fetch_ical_text(config_data: ConfigInfo) {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(config_data.cal_url)
        .basic_auth(config_data.cal_username, Some(config_data.cal_pass))
        .send()
        .expect("failed to fetch ical");
    let school = File::create("school.ics").unwrap();
    let mut writer = BufWriter::new(school);
    writer
        .write_all(String::from_utf8_lossy(response.text().unwrap().as_bytes()).as_bytes())
        .expect("failed to write ics");
}
fn clean_ics_value(v: String) -> String {
    v.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}
fn parse_text_blocks() -> Vec<TimeBlock> {
    let mut timeblock_vec = vec![];
    let buf = BufReader::new(File::open("school.ics").expect("couldn't open school.ics"));
    let icalparser = IcalParser::new(buf);

    for calendar in icalparser {
        let calendar_parse = calendar.expect("failed to parse calendar");

        for event in calendar_parse.events {
            let mut start = String::new();
            let mut duration = String::new();
            let mut uid = String::new();
            let mut summary = String::new();
            let mut dtstamp = String::new();

            for property in event.properties {
                match property.name.as_str() {
                    "DTSTART" => start = clean_ics_value(property.value.unwrap_or_default()),
                    "DURATION" => duration = clean_ics_value(property.value.unwrap_or_default()),
                    "UID" => uid = clean_ics_value(property.value.unwrap_or_default()),
                    "SUMMARY" => summary = clean_ics_value(property.value.unwrap_or_default()),
                    "DTSTAMP" => dtstamp = clean_ics_value(property.value.unwrap_or_default()),
                    _ => {}
                }
            }

            let block = TimeBlockRaw {
                dtstart: start,
                duration,
                uid,
                summary,
                dtstamp,
            };

            timeblock_vec.push(block);
        }
    }

    convert_raw_blocks(timeblock_vec)
}

fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = Mutex::new(fetch_tasks());
    fetch_ical_text(
        config_data
            .lock()
            .expect("failed to unlock Mutex")
            .auth
            .clone(),
    );
    let blocks_vec = Mutex::new(parse_text_blocks());
    schedule(
        tasks,
        config_data
            .lock()
            .expect("failed to unlock mutex")
            .auth
            .clone(),
        blocks_vec,
    );
}
