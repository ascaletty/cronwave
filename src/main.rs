use chrono::Utc;
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone};
use icalendar::{Component, DatePerhapsTime, Event, EventLike};
use reqwest::blocking::{self, Client};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::Deserialize;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, PipeReader, Read, Write};
use std::ops::Deref;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::str::FromStr;
use std::string;
use std::sync::{Mutex, MutexGuard};
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
    status: String,
    urgency: f32,
}

#[derive(Debug, Deserialize)]
pub struct RawTask {
    uuid: String,
    description: String,
    due: Option<String>,
    estimated: Option<String>,
    status: String,
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

#[derive(Debug, Deserialize, Clone)]
pub struct Gap {
    //we are going to store our start and ends as unix timestamps
    start: i64,
    end: i64,
}
impl Gap {
    fn last(last_block: i64, greatest_dur: i64) -> Self {
        Self {
            start: last_block,
            end: greatest_dur,
        }
    }
}
impl TimeBlock {
    fn last(last_block: i64, greatest_duration: i64) -> Self {
        Self {
            duration: Duration::seconds(greatest_duration),
            dtstart: {
                let dt = NaiveDateTime::from_timestamp(last_block, 0);
                Local::from_utc_datetime(&Local, &dt)
            },
            summary: "last".to_string(),
            uid: "615d0917-2955-4f3e-ae21-ca0f72bdc48a".to_string(),
            dtstamp: Utc::now(),
        }
    }
}

fn create_caldav_events(
    config_data: ConfigInfo,
    blocks: MutexGuard<'_, Vec<TimeBlock>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut string_vec = vec!["BEGIN:VCALENDAR".to_string()];

    for event in blocks.iter() {
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
            dtstart = event.dtstart.format("%Y%m%dT%H%M%S"),
            duration = ical::format_duration_ical(event.duration),
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
fn find_the_gaps(blocks: Vec<TimeBlock>, greatest_dur_task: i64) -> Vec<Gap> {
    let last_block = blocks.last().unwrap().dtstart.timestamp();
    let mut gap_vec = vec![];
    let mut i = 0;
    for block in blocks.clone() {
        i += 1;
        let gap = Gap {
            start: block.dtstart.timestamp() + block.duration.num_seconds(),
            end: blocks
                .get(i)
                .unwrap_or(&TimeBlock::last(last_block, greatest_dur_task))
                .dtstart
                .timestamp(),
        };
        print!(
            "new gap at {} to {}",
            Local::timestamp_opt(&Local, gap.start, 0).unwrap(),
            Local::timestamp_opt(&Local, gap.end, 0).unwrap()
        );
        gap_vec.push(gap);
    }
    gap_vec
}
fn schedule(tasks: Mutex<Vec<Task>>, config_data: ConfigInfo, blocks: Mutex<Vec<TimeBlock>>) {
    let mut block_guard = blocks.lock().unwrap();

    let mut tasks_copy = {
        let task_guard = tasks.lock().unwrap();
        task_guard.clone()
    };
    let greatest_dur = tasks_copy.last().unwrap().estimated.num_seconds();

    block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    // println!("block gaurs \n \n \n {:?}", block_guard);

    let time_line = Local::now().timestamp();

    let mut blocks_copy = block_guard.clone();

    blocks_copy.retain(|x| x.dtstart.timestamp() + x.duration.num_seconds() > time_line);
    let mut gaps = find_the_gaps(blocks_copy, greatest_dur);
    let num_of_tasks = tasks_copy.len();

    let mut scheduled = vec![];
    gaps.retain(|x| x.end > time_line);
    tasks_copy.sort_by_key(|x| x.due.timestamp());
    // println!("tasks copy \n {:?} \n", tasks_copy);
    for gap in gaps {
        let mut time_til = gap.end - gap.start;
        let mut block_start = gap.start;
        // println!("time til= {}", time_til / 60);

        while let Some((idx, _)) = tasks_copy
            .iter()
            .enumerate()
            .filter(|(_, t)| t.status != "scheduled" && t.estimated.num_seconds() <= time_til)
            .max_by_key(|(_t, t)| t.estimated.num_seconds())
        {
            let task = tasks_copy[idx].clone();
            if block_start + task.estimated.num_seconds() > task.due.timestamp() {
                println!("Task will not be completed in time");
            }
            scheduled.push(TimeBlock {
                duration: task.estimated,
                dtstart: Local.timestamp_opt(block_start, 0).unwrap(),
                uid: task.uuid.clone(),
                dtstamp: Utc::now(),
                summary: task.description.clone(),
            });
            tasks_copy[idx].status = "scheduled".to_string();

            // Advance inside the block
            block_start += task.estimated.num_seconds();
            time_til -= task.estimated.num_seconds();
        }
    }

    tasks_copy.retain(|x| x.status == "pending".to_string());
    for block in scheduled {
        block_guard.push(block);
    }
    let last_time_scheduled = block_guard.last().unwrap().dtstart;
    let mut time_line_after = last_time_scheduled;
    for task in tasks_copy {
        if time_line_after.timestamp() > task.due.timestamp() {
            println!("task will not be completed in time");
        }
        block_guard.push(TimeBlock {
            duration: task.estimated,
            dtstart: time_line_after,
            uid: task.uuid,
            dtstamp: Utc::now(),
            summary: task.description.clone(),
        });
        //update time_line_after
        time_line_after += task.estimated
    }

    // block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    // println!("{:?}", block_guard);
    match create_caldav_events(config_data, block_guard) {
        Ok(_) => {
            println!("Events created!");
            let mut i = 0;
            while i < num_of_tasks {
                let delete_string = format!("task delete 1");
                Command::new(delete_string).exec();
                Command::new("y").exec();
                i += 1;
            }
        }
        Err(_) => {
            println!("events not created")
        }
    }
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
    // println!("TASKS OUTPUT");
    // println!("output of fetch tasks{:?}", output);

    output
}
fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = Mutex::new(fetch_tasks());
    // println!("\n, \n, TASKS, \n, {:?}", tasks);
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
