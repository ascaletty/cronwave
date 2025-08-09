use chrono::Utc;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use ical::IcalParser;
use iso8601::Duration as isoDuration;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::json;
use std::fs::{self, read_to_string, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::process::Command;
use std::str::FromStr;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use uuid::Uuid;

use crate::config::ConfigInfo;
mod config;
#[derive(Debug)]
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

#[derive(Debug, Deserialize)]
pub struct TimeBlock {
    dtstart: chrono::DateTime<Local>,
    duration: Duration,
    uid: String,
    summary: String,
    dtstamp: chrono::DateTime<Local>,
}

fn create_caldav_event(
    start_time: chrono::DateTime<Local>,
    duration_minutes: Duration,
    summary: &str,
    config_data: ConfigInfo,
) -> Result<(), Box<dyn std::error::Error>> {
    // Generate UID and calculate end time
    let uid = Uuid::new_v4().to_string();
    let end_time = start_time + duration_minutes;

    let blocking =
        File::open("blockin.json").expect("failed to open blocking.json to write new block");
    let blocking_read = File::open("blockin.json").expect("failed to open blockin.json");
    let mut writer = BufWriter::new(blocking);
    let mut json_reader = BufReader::new(blocking_read);
    let mut json_data = vec![];
    json_reader
        .read_to_end(&mut json_data)
        .expect("failed json read");
    let mut parsed_json: Vec<serde_json::Value> =
        serde_json::from_slice(&json_data).expect("failed to parse json");
    let new_block = json!({
        "dtstart": start_time,
        "end": end_time,
        "uid": uid,
        "summary": summary.to_string(),
        "dtstamp": Utc::now().to_string(),
    });
    parsed_json.push(new_block);
    let json_string = serde_json::to_string_pretty(&parsed_json).expect("failed to render json");
    println!("NEW BLOCK{json_string}");
    writer
        .write_all(json_string.as_bytes())
        .expect("failed to write new_block to json");
    let json_data = fs::read_to_string("blockin.json").expect("could find/read blocking.json");
    let blocks: Vec<TimeBlockRaw> =
        serde_json::from_str(&json_data).expect("failed to read json_data");
    let mut block_serialized = convert_raw_blocks(blocks);

    block_serialized.sort_by_key(|x| x.dtstart);
    let mut string_vec = vec!["BEGIN:VCALENDAR\r".to_string()];
    for event in block_serialized {
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
    println!("{}", combined);

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
            let total_minutes = (*year as i64 * 365 * 24 * 60) +     // assume 365 days/year
                (*month as i64 * 30 * 24 * 60) +     // assume 30 days/month
                (*day as i64 * 24 * 60) +
                (*hour as i64 * 60) +
                (*minute as i64) +
                (*second as i64 / 60);
            total_minutes
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
        panic!("Invalid iCalendar datetime: {}", s);
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
        DateTime::from_str(&format!("{}Z", rfc)).expect("failed to get datetime")
    } else {
        Local
            .from_local_datetime(&NaiveDateTime::from_str(&rfc).unwrap())
            .unwrap()
    }
}
fn schedule(tasks: Vec<Task>, config_data: ConfigInfo) {
    let json_data = fs::read("blockin.json").expect("could find/read blocking.json");
    let json_clean = String::from_utf8_lossy(&json_data);
    let blocks: Vec<TimeBlockRaw> =
        serde_json::from_str(&json_clean).expect("failed to read json_data");
    let mut serialized_blocks = convert_raw_blocks(blocks);
    let mut not_scheduled = vec![];
    for block in serialized_blocks.iter() {
        let current_time = Local::now();
        let time_til = serialized_blocks.iter().next().unwrap().dtstart - current_time;
        for task in tasks.iter() {
            if task.estimated <= time_til {
                create_caldav_event(
                    block.dtstart,
                    task.estimated,
                    &task.description,
                    config_data.clone(),
                )
                .expect("failed to create_caldav_event");
            } else {
                not_scheduled.push(task);
            }
        }
    }
    let end_time_block = serialized_blocks.pop().unwrap();
    let mut end_time = end_time_block.dtstart + end_time_block.duration;
    for notime in not_scheduled {
        create_caldav_event(
            end_time,
            notime.estimated,
            &notime.description,
            config_data.clone(),
        )
        .expect("failed to create_caldav_event");
        end_time += notime.estimated;
    }
}
fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Input: "20250806T195556Z" → Output: DateTime<Utc>
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|ndt| DateTime::<Utc>::from_utc(ndt, Utc))
}

fn parse_duration(iso: &str) -> iso8601::Duration {
    // Input: "PT1H" → Output: chrono::Duratio
    iso.parse::<iso8601::Duration>()
        .expect("error parsing iso8601")
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
    let mut school = File::create("school.ics").unwrap();
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
fn parse_text_blocks() -> String {
    let mut json_vec = vec![];
    let buf = BufReader::new(File::open("school.ics").expect("couldn't open school.ics"));
    let icalparser = IcalParser::new(buf);

    for calendar in icalparser {
        let calendar_parse = calendar.expect("failed to parse calendar");

        for event in calendar_parse.events {
            let mut start = String::new();
            let mut end = String::new();
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

            let block_json = json!({
                "dtstart": start,
                "duration": duration,
                "uid": uid,
                "summary": summary,
                "dtstamp": dtstamp,
            });

            json_vec.push(block_json);
        }
    }

    serde_json::to_string_pretty(&json_vec)
        .unwrap_or_else(|_| "failed json_string parse".to_string())
}
fn write_json(json: String) {
    let mut file = File::create("blockin.json").expect("couldnt create blockin.json");
    file.write_all(json.as_bytes());
}

fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = fetch_tasks();
    fetch_ical_text(
        config_data
            .lock()
            .expect("failed to unlock Mutex")
            .auth
            .clone(),
    );
    let blocks_text = parse_text_blocks();
    write_json(blocks_text);
    schedule(
        tasks,
        config_data
            .lock()
            .expect("failed to unlock mutex")
            .auth
            .clone(),
    );
}
