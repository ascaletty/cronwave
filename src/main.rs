use chrono::Utc;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use ical::IcalParser;
use iso8601::Duration as isoDuration;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, PipeReader, Read, Write};
use std::ops::Deref;
use std::process::Command;
use std::str::FromStr;
use std::string;
use std::sync::Mutex;
use uuid::Uuid;

use crate::config::ConfigInfo;
struct Calendar {
    vevent_vec: Vec<TimeBlock>,
}
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

#[derive(Debug)]
enum EventTypes {
    VEVENT,
    VTODO,
    VALARM,
    VCALENDAR,
}
#[derive(Debug)]
struct StringBlock {
    name: EventTypes,
    block: Vec<String>,
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
    config_data: ConfigInfo,
    blocks: Mutex<Vec<TimeBlock>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut string_vec = vec!["BEGIN:VCALENDAR".to_string()];

    println!("scheduled{:?}", scheduled);

    for event in scheduled {
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

    // UTC (ends with 'Z'), e.g. "20250806T195556Z"
    if s.ends_with('Z') {
        let core = &s[..s.len() - 1]; // drop the Z
        let naive = NaiveDateTime::parse_from_str(core, "%Y%m%dT%H%M%S")
            .expect(&format!("invalid UTC datetime: {}", s));
        // parse as UTC then convert to Local (important!)
        let utc_dt = DateTime::<Utc>::from_utc(naive, Utc);
        return utc_dt.with_timezone(&Local);
    }

    // Floating/local datetime (no Z), e.g. "20250806T160000"
    if s.len() == 15 && s.contains('T') {
        let naive = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S")
            .expect(&format!("invalid local datetime: {}", s));
        // from_local_datetime returns a LocalResult; .single() ensures we get a single mapping
        return Local
            .from_local_datetime(&naive)
            .single()
            .expect("ambiguous or nonexistent local datetime");
    }

    // All-day DATE (YYYYMMDD)
    if s.len() == 8 {
        let naive_date = NaiveDate::parse_from_str(s, "%Y%m%d")
            .expect(&format!("invalid date-only DTSTART: {}", s));
        let naive_dt = naive_date.and_hms(0, 0, 0);
        return Local
            .from_local_datetime(&naive_dt)
            .single()
            .expect("ambiguous local date");
    }

    panic!("Unsupported iCalendar datetime format: '{}'", s);
}

fn schedule(tasks: Mutex<Vec<Task>>, config_data: ConfigInfo, blocks: Mutex<Vec<TimeBlock>>) {
    let blocks_copy = {
        let guard = blocks.lock().unwrap();
        guard.clone()
    };
    print!("blocks{:?}", blocks_copy);

    let mut scheduled = vec![];
    let tasks_copy = {
        let guard = tasks.lock().unwrap();
        guard.clone()
    };
    println!("TASKS COPY");

    println!("tasks_copy mutex gaurd {:?}", tasks_copy);
    println!("");
    for block in &blocks_copy {
        let current_time = Local::now();
        let time_til = block.dtstart - current_time;

        let mut i = 0;
        for task in tasks_copy.clone() {
            if task.estimated <= time_til {
                scheduled.push(TimeBlock {
                    duration: task.estimated,
                    dtstart: block.dtstart,
                    uid: task.uuid,
                    dtstamp: Local::now(),
                    summary: task.description.clone(),
                });
                tasks.lock().unwrap().remove(i);
            }
            i += 1;
        }
    }
    let last_block_time = {
        let guard = blocks.lock().unwrap();
        let last = guard
            .last()
            .unwrap_or(&TimeBlock {
                dtstart: Local::now(),
                dtstamp: Local::now(),
                duration: Duration::zero(),
                summary: "".to_string(),
                uid: Uuid::new_v4().to_string(),
            })
            .dtstart;
        last + guard
            .last()
            .unwrap_or(&TimeBlock {
                dtstart: Local::now(),
                dtstamp: Local::now(),
                duration: Duration::zero(),
                summary: "".to_string(),
                uid: Uuid::new_v4().to_string(),
            })
            .duration
    };
    for task_left in tasks.lock().unwrap().iter() {
        scheduled.push(TimeBlock {
            duration: task_left.estimated,
            dtstart: last_block_time,
            uid: task_left.uuid.clone(),
            dtstamp: Local::now(),
            summary: task_left.description.clone(),
        });
    }
    create_caldav_events(scheduled, config_data, blocks).expect("failed to create_caldav_events");
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
    println!("TASKS OUTPUT");
    println!("output of fetch tasks{:?}", output);

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
    // println!("Writen the ics{:?}", response.text());
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
fn parse_text_blocks() -> Vec<Vec<String>> {
    print!("can we get much higher");

    let contents = fs::read_to_string("school.ics").expect("failed to read school.ics");
    let mut start = String::new();
    let mut duration = String::new();
    let mut summary = String::new();
    let mut dtstamp = String::new();
    let mut uid = String::new();
    let mut splits = vec![];
    let mut i = 0;
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split into key and value by first ':'
        let mut parts = line.splitn(2, ':');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        match key {
            "BEGIN" => {
                let start_event = value.to_string();
                let formatted_string = format!("B:{}:{}", i, start_event);
                splits.push(formatted_string);
            }
            "END" => {
                let end_event = value.to_string();
                let formatted_string = format!("E:{}:{}", i, end_event);
                splits.push(formatted_string);
            }
            _ => print!(""),
        }
        i += 1;
    }
    println!("splits{:?}", splits);
    let mut sections: Vec<StringBlock> = vec![];
    let mut splits_iter = splits.iter();

    for line in splits {
        let event_val: Vec<&str> = line.split(':').collect();
        if let start_or_end = event_val.get(0).unwrap() == "B" {
            let event_start: usize = event_val.iter().nth(1).unwrap().parse().unwrap();
            let event_name_raw = event_val.iter().nth(2).unwrap();
            let event_name = match event_name_raw.deref() {
                "VCALENDAR" => EventTypes::VCALENDAR,
                "VEVENT" => EventTypes::VEVENT,
                "VTODO" => EventTypes::VTODO,
                "VALARM" => EventTypes::VALARM,
                _ => panic!("unknown event type"),
            };
            let event_end = splits_iter
                .clone()
                .position(|x| {
                    x.split(":").into_iter().nth(2).unwrap() == event_name_raw.to_string()
                })
                .expect("couldnt find matching event end");
            let block = contents
                .lines()
                .take(event_end - event_start)
                .map(|line| line.to_string())
                .collect();
            let new_block = StringBlock {
                name: event_name,
                block,
            };
            sections.push(new_block);
        }
    }
    let mut vevent_vec = vec![];
    println!("sections{:?}", sections);
    for section in sections {
        match section.name {
            EventTypes::VEVENT => vevent_vec.push(section.block),
            EventTypes::VCALENDAR => println!("found a calendar"),
            EventTypes::VTODO => println!("found a todo"),
            EventTypes::VALARM => println!("found an alarm"),
        }
    }
    vevent_vec
}
fn parse_vevent(event_vec: Vec<Vec<String>>) -> Mutex<Vec<TimeBlock>> {
    let mut timeblock_vec_raw = vec![];
    println!("event vec strings{:?}", event_vec);
    let mut start = String::new();
    let mut summary = String::new();
    let mut uid = String::new();
    let mut duration = String::new();
    let mut dtstamp = String::new();
    let block_mutex: Mutex<Vec<TimeBlock>> = Mutex::new(Vec::new());
    for event in event_vec {
        for line in event.iter() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Split into key and value by first ':'
            let mut parts = line.splitn(2, ':');
            let key = parts.next().unwrap_or("");
            let value = parts.next().unwrap_or("");

            match key {
                "DTSTART" => start = value.to_string(),
                "SUMMARY" => summary = value.to_string(),
                "UID" => uid = value.to_string(),
                "DURATION" => duration = value.to_string(),
                "DTSTAMP" => dtstamp = value.to_string(),
                _ => println!("Other key: {}, value: {}", key, value),
            }
        }
        let new_block = TimeBlockRaw {
            dtstart: start.clone(),
            summary: summary.clone(),
            uid: uid.clone(),
            duration: duration.clone(),
            dtstamp: dtstamp.clone(),
        };
        timeblock_vec_raw.push(new_block);
    }
    let serialized_block_vec = convert_raw_blocks(timeblock_vec_raw);
    for block in serialized_block_vec {
        block_mutex.lock().unwrap().push(block);
    }
    println!("BLOCK Mutex \n");
    println!("{:?}", block_mutex);
    block_mutex
}

fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = Mutex::new(fetch_tasks());
    println!("\n, \n, TASKS, \n, {:?}", tasks);
    fetch_ical_text(
        config_data
            .lock()
            .expect("failed to unlock Mutex")
            .auth
            .clone(),
    );
    let vevent_string_vec = parse_text_blocks();
    let vevent_mutex = parse_vevent(vevent_string_vec);
    schedule(
        tasks,
        config_data
            .lock()
            .expect("failed to unlock mutex")
            .auth
            .clone(),
        vevent_mutex,
    );
}
