use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use cronwave_lib::structs::Rrule;
use cronwave_lib::structs::{auth, ConfigInfo, Gap, Task, TimeBlock, TimeBlockRaw};
use reqwest::blocking::Client;
use reqwest::header::*;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
fn create_virtual_events(blocks: Vec<TimeBlock>) -> Vec<TimeBlock> {
    let current_time = Local::now().timestamp();
    let mut with_virtual = vec![];
    for block in blocks {
        let mut time_remaining = block.rrule.until - current_time;
        let mut i = 1;
        while time_remaining > 0 {
            let freq = match block.clone().rrule.freq.as_str() {
                "WEEKLY" => 604800,
                "DAILY" => 86400,
                "MONTHLY" => 2628002,
                "YEARLY" => 31536000,
                _ => panic!("unrecognized rrule type"),
            };
            let new_block = TimeBlock {
                rrule: Rrule {
                    freq: "NONE".to_string(),
                    until: 0,
                },
                dtstart: chrono::DateTime::from_timestamp(block.dtstart.timestamp() + i * freq, 0)
                    .unwrap()
                    .into(),
                duration: block.duration,
                summary: block.summary.clone(),
                dtstamp: block.dtstamp,
                uid: block.uid.clone(),
            };
            i += 1;
            time_remaining -= freq;
            with_virtual.push(new_block);
        }
    }
    with_virtual
}
pub fn schedule(tasks: Mutex<Vec<Task>>, config_data: auth, blocks: Mutex<Vec<TimeBlock>>) {
    let mut block_guard = blocks.lock().unwrap();

    let mut tasks_copy = {
        let task_guard = tasks.lock().unwrap();
        task_guard.clone()
    };
    let greatest_dur = tasks_copy.last().unwrap().estimated.num_seconds();
    let mut repeating_clone = block_guard.clone();
    repeating_clone.retain(|x| x.rrule.freq != "NONE");
    let mut with_virtual = create_virtual_events(repeating_clone);

    with_virtual.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    // println!("block gaurs \n \n \n {:?}", block_guard);

    let time_line = Local::now().timestamp();

    let mut blocks_copy = with_virtual.clone();

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
                rrule: Rrule {
                    freq: "NONE".to_string(),
                    until: 0,
                },
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
            rrule: Rrule {
                freq: "NONE".to_string(),
                until: 0,
            },
        });
        //update time_line_after
        time_line_after += task.estimated
    }

    // block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    // println!("{:?}", block_guard);
    match create_caldav_events(config_data, block_guard) {
        Ok(_) => {
            println!("Events created!");
            Command::new("task delete all").exec();
            Command::new("y").exec();
            Command::new("all").exec();
        }
        Err(_) => {
            println!("events not created")
        }
    }
}

fn find_the_gaps(blocks: Vec<TimeBlock>, greatest_dur_task: i64) -> Vec<Gap> {
    let last_block = blocks
        .last()
        .unwrap_or(&TimeBlock::last(
            Local::now().timestamp(),
            greatest_dur_task,
        ))
        .dtstart
        .timestamp();
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
fn create_caldav_events(
    config_data: auth,
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
RRULE:FREQ={rrule_freq};UNTIL={rrule_until}\r
END:VEVENT\r",
            rrule_freq = event.rrule.freq,
            rrule_until = DateTime::from_timestamp(event.rrule.until, 0)
                .unwrap()
                .format("%Y%m%dT%H%M%S"),
            uid = event.uid,
            now = event.dtstamp.format("%Y%m%dT%H%M%SZ"),
            dtstart = event.dtstart.format("%Y%m%dT%H%M%S"),
            duration = cronwave_lib::ical::format_duration_ical(event.duration),
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
