use chrono::Utc;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use icalendar::{Component, DatePerhapsTime, Event, EventLike};
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
mod ical;
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

#[derive(Debug)]
pub struct TimeBlockRaw {
    dtstart: DatePerhapsTime,
    duration: String,
    uid: String,
    summary: String,
    dtstamp: DateTime<Utc>,
}
#[derive(Debug, Deserialize, Clone)]
pub struct TimeBlock {
    dtstart: chrono::DateTime<Local>,
    duration: Duration,
    uid: String,
    summary: String,
    dtstamp: chrono::DateTime<Utc>,
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
            duration = iso8601::Duration::YMDHMS {
                year: 0,
                month: 0,
                day: 0,
                hour: u32::from_str(&event.duration.num_hours().to_string()).unwrap_or(0),
                minute: 0,
                second: 0,
                millisecond: 0,
            }
            .to_string(),
            summary = event.summary,
        )
        .to_string();
        string_vec.push(ics);
    }
    string_vec.push("END:VCALENDAR\r".to_string());
    let combined = string_vec.join("\n");
    println!("combined: \n {combined}");

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
    for block in &blocks_copy {
        let current_time = Local::now();
        println!("time is {}", current_time);
        let time_til = block.dtstart - current_time;

        let mut i = 0;
        for task in tasks_copy.clone() {
            if task.estimated <= time_til {
                scheduled.push(TimeBlock {
                    duration: task.estimated,
                    dtstart: block.dtstart,
                    uid: task.uuid,
                    dtstamp: Utc::now(),
                    summary: task.description.clone(),
                });
                if tasks.lock().unwrap().len() > 1 {
                    tasks.lock().unwrap().remove(i);
                }
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
                dtstamp: Utc::now(),
                duration: Duration::zero(),
                summary: "".to_string(),
                uid: Uuid::new_v4().to_string(),
            })
            .dtstart;
        last + guard
            .last()
            .unwrap_or(&TimeBlock {
                dtstart: Local::now(),
                dtstamp: Utc::now(),
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
            dtstamp: Utc::now(),
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
            estimated: Duration::minutes(ical::duration_to_minutes(
                &iso8601::duration(&task.estimated.unwrap())
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
fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = Mutex::new(fetch_tasks());
    println!("\n, \n, TASKS, \n, {:?}", tasks);
    ical::fetch_ical_text(
        config_data
            .lock()
            .expect("failed to unlock Mutex")
            .auth
            .clone(),
    );
    let timeblock_mutex = ical::parse_ical_blocks();
    schedule(
        tasks,
        config_data
            .lock()
            .expect("failed to unlock mutex")
            .auth
            .clone(),
        timeblock_mutex,
    );
}
