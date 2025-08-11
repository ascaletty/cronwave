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

fn schedule(tasks: Mutex<Vec<Task>>, config_data: ConfigInfo, blocks: Mutex<Vec<TimeBlock>>) {
    let mut block_guard = blocks.lock().unwrap();

    let mut tasks_copy = {
        let task_guard = tasks.lock().unwrap();
        task_guard.clone()
    };

    block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));

    let blocks_copy = block_guard.clone();

    let mut time_line = Local::now();
    let mut blocks_copy_iter = blocks_copy.iter().clone();
    let mut not_scheduled = vec![];
    for task in tasks_copy {
        println!("scheduling task {}", task.description);
        let next_task_estimated= tasks_copy.iter().next().unwrap();
        for block in blocks_copy_iter.clone() {
            println!("trying block {}", block.summary);
            let mut time_til = time_line - block.dtstart;
            let estimated_time_task = task.estimated;
            let next_block_time=blocks_copy_iter.next().unwrap_or(a).dtstart;
            // if there is no time til then we need to update te time_line to the end of the block
            // and update the time_til to be the gap between the end of that block and then next
            // one
            if time_til > Duration::zero() {
                //this should only run when we are actively in a block
                //which should be only once at the beginning
                time_line = block.dtstart + block.duration;
                time_til = blocks_copy_iter.next().unwrap().dtstart - time_line;
            }
            //if the there is time to do the task lets schedule it
            if estimated_time_task <= time_til {
                block_guard.push(TimeBlock {
                    duration: estimated_time_task,
                    dtstart: time_line,
                    uid: task.uuid.clone(),
                    dtstamp: Utc::now(),
                    summary: task.description.clone(),
                });
                time_line += estimated_time_task;
                if 
            }
            //otherwise put it in the not scheduled vec
            else {
                not_scheduled.push(task.clone());
                time_line += estimated_time_task;
            }
        }
    }
    //once we are done looping through all the tasks we are left with the scheduled and the not
    //scheduled. From here we need to get the time of the last scheduled task or block
    //and then we take the not scheduled tasks and schedule them all one after another in the free
    //time
    let last_time_scheduled = block_guard.last().unwrap().dtstart;
    let mut time_line_after = last_time_scheduled;
    for task in not_scheduled {
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

    block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    println!("{:?}", block_guard);
    create_caldav_events(config_data, block_guard).expect("failed to create_caldav_events");
}

// for block in &blocks_copy {
//     println!("time is {}", current_time);
//     if j == 0 {
//         time_til = block.dtstart - current_time;
//     } else {
//         time_til = blocks_copy.iter().next().unwrap().dtstart - current_time;
//     }
//     //if there is still time_til the next block ie we are not actively in a block
//     //then create tasks
//     if time_til > Duration::zero() {
//         println!("in if case if the time_til >0{time_til}");
//         let mut i = 0;
//         for task in tasks_copy.clone() {
//             //if theres time to fit the task in , then schedule it, if not then dont
//             if task.estimated <= time_til {
//                 scheduled.push(TimeBlock {
//                     duration: task.estimated,
//                     dtstart: current_time,
//                     uid: task.uuid,
//                     dtstamp: Utc::now(),
//                     summary: task.description.clone(),
//                 });
//                 tasks.lock().unwrap().iter_mut().nth(i).unwrap().status =
//                     "scheduled".to_string();
//                 //update current_time
//                 current_time += task.estimated;
//                 println!("updated current_time {current_time}");
//                 //update time_til
//                 time_til -= task.estimated;
//             }
//             i += 1;
//         }
//     }
//     //otherwise if we are in a block set the "current_time" to the end of the block
//     else {
//         current_time = block.dtstart + block.duration;
//         println!(
//             "in the else case the time should be set to the end of the block{current_time}"
//         );
//         println!("time til: {time_til}");
//         let mut i = 0;
//         for task in tasks_copy.clone() {
//             println!("current_time in the for task block");
//             if task.estimated <= time_til {
//                 scheduled.push(TimeBlock {
//                     duration: task.estimated,
//                     dtstart: current_time,
//                     uid: task.uuid,
//                     dtstamp: Utc::now(),
//                     summary: task.description.clone(),
//                 });
//
//                 tasks.lock().unwrap().iter_mut().nth(i).unwrap().status =
//                     "scheduled".to_string();
//                 //update current_time
//                 current_time += task.estimated;
//                 //update time_til
//                 time_til -= task.estimated;
//             }
//             i += 1;
//         }
//
// remove the tasks from the global task vec

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
