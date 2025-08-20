use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use cronwave::structs::*;
use iso8601::duration;
use reqwest::blocking::Client;
use reqwest::header::*;
use rrule::RRule;
use rrule::RRuleSet;
use std::fmt::format;
use std::fs::File;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::Command;

use std::io::Write;
use std::process::Stdio;
use std::task;

fn mark_all_tasks_scheduled() {
    let count = Command::new("task")
        .arg("+unscheduled")
        .arg("status:pending")
        .arg("count")
        .output()
        .expect("failed to count tasks");
    let number: i32 = std::str::from_utf8(&count.stdout)
        .expect("invalid UTF-8")
        .trim()
        .parse()
        .expect("not a valid integer");
    for i in 1..number + 1 {
        print!("i:{}", i);
        let string = format!("task modify {} +scheduled -unscheduled", i);
        println!("{string}");
        let result = Command::new("task")
            .arg("modify")
            .arg(i.to_string())
            .arg("+scheduled")
            .arg("-unscheduled")
            .status()
            .expect("failed to run command");
    }
}

pub fn schedule(tasks: Vec<Task>, config_data: ConfigInfo, mut blocks: Vec<TimeBlock>) {
    let mut tasks_copy = tasks.clone();

    // with_virtual.sort_by(|a, b| a.dtstart.cmp(&b.dtstart));
    let time_line = Local::now().timestamp();

    blocks.retain(|x| {
        x.dtstart + x.duration.unwrap_or(0) > time_line
            || x.dtend.unwrap_or(0) > time_line
            || x.rrule.is_some()
    });
    let mut gaps = find_the_gaps(blocks.clone());

    gaps.retain(|x| x.end > time_line);
    tasks_copy.sort_by_key(|x| x.due);
    // println!("tasks copy \n {:?} \n", tasks_copy);
    for gap in gaps {
        let mut time_til = gap.end - gap.start;
        let mut block_start = gap.start;

        // println!("time til= {}", time_til / 60);

        while let Some((idx, _)) = tasks_copy
            .iter()
            .enumerate()
            .filter(|(_, t)| t.status != "scheduled" && t.estimated <= time_til)
            .max_by_key(|(_t, t)| t.estimated)
        {
            let task = tasks_copy[idx].clone();
            if block_start + task.estimated > task.due {
                println!("Task will not be completed in time");
            }
            blocks.push(TimeBlock {
                duration: Some(task.estimated),
                dtstart: block_start,
                dtend: None,
                uid: task.uuid.clone(),
                dtstamp: Utc::now(),
                summary: task.description.clone(),
                rrule: None,
            });
            tasks_copy[idx].status = "scheduled".to_string();

            // Advance inside the block
            block_start += task.estimated;
            time_til -= task.estimated;
        }
    }

    tasks_copy.retain(|x| x.status == "pending".to_string());
    let last_time_scheduled =
        blocks.last().unwrap().dtstart + blocks.last().unwrap().duration.unwrap();
    let mut time_line_after = last_time_scheduled;
    for task in tasks_copy {
        if time_line_after > task.due {
            println!("task will not be completed in time");
        }
        blocks.push(TimeBlock {
            duration: Some(task.estimated),
            dtstart: time_line_after,
            dtend: None,
            rrule: None,
            uid: task.uuid,
            dtstamp: Utc::now(),
            summary: task.description.clone(),
        });
        //update time_line_after
        time_line_after += task.estimated
    }

    match create_caldav_events(config_data, blocks) {
        Ok(_) => {
            println!("Events created!");
            mark_all_tasks_scheduled();
        }
        Err(_) => {
            println!("events not created")
        }
    }
}
fn find_the_gaps(blocks: Vec<TimeBlock>) -> Vec<Gap> {
    let mut gap_vec = vec![];

    // Expand recurrences into actual blocks first
    let mut expanded_blocks: Vec<TimeBlock> = vec![];
    for block in blocks {
        if let Some(rrule) = block.rrule.clone() {
            // Use block.dtstart as the seed
            let tz = rrule::Tz::Local(Local);
            let start_tz = Local
                .timestamp_opt(block.dtstart, 0)
                .unwrap()
                .with_timezone(&tz);

            let rruleset = RRuleSet::new(start_tz).rrule(rrule);
            for time in rruleset.all_unchecked() {
                let mut b = block.clone();
                b.dtstart = time.timestamp();
                // shift dtend if duration is defined
                if let Some(dur) = b.duration {
                    b.dtend = Some(b.dtstart + dur);
                }
                expanded_blocks.push(b);
            }
        } else {
            expanded_blocks.push(block);
        }
    }

    expanded_blocks.sort_by_key(|b| b.dtstart);
    let now = Local::now().timestamp();
    if let Some(first) = expanded_blocks.first() {
        if now < first.dtstart {
            gap_vec.push(Gap {
                start: now,
                end: first.dtstart,
            });
        }
    }

    // Walk through and find gaps between consecutive blocks
    for w in expanded_blocks.windows(2) {
        let current = &w[0];
        let next = &w[1];

        let current_end = match current.duration {
            Some(dur) => current.dtstart + dur,
            None => current.dtend.unwrap_or(current.dtstart),
        };

        if current_end < next.dtstart {
            gap_vec.push(Gap {
                start: current_end,
                end: next.dtstart,
            });
        }
    }
    gap_vec
}

fn create_caldav_events(
    config_data: ConfigInfo,
    blocks: Vec<TimeBlock>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut string_vec = vec!["BEGIN:VCALENDAR".to_string()];

    for event in blocks {
        string_vec.push("BEGIN:VEVENT".to_string());
        string_vec.push(format!(
            "DTSTART:{}",
            Local::timestamp_opt(&Local, event.dtstart, 0)
                .unwrap()
                .format("%Y%m%dT%H%M%S")
        ));

        string_vec.push(format!("UID:{}", event.uid));
        string_vec.push(format!(
            "DTSTAMP:{}",
            event.dtstamp.format("%Y%m%dT%H%M%SZ")
        ));
        match event.duration {
            Some(dur) => {
                let hours = chrono::Duration::seconds(dur).num_hours() * 3600;
                let minutes = chrono::Duration::seconds(dur - hours).num_minutes();
                if hours == 0 {
                    string_vec.push(format!(
                        "DURATION:PT{}M",
                        chrono::Duration::seconds(dur).num_minutes()
                    ))
                } else if minutes == 0 {
                    string_vec.push(format!(
                        "DURATION:PT{}H",
                        chrono::Duration::seconds(dur).num_hours()
                    ))
                } else {
                    string_vec.push(format!(
                        "DURATION:PT{}H{}M",
                        chrono::Duration::seconds(dur).num_hours(),
                        chrono::Duration::seconds(dur - hours).num_minutes()
                    ))
                }
            }
            None => (),
        };
        match event.dtend {
            Some(dtend) => string_vec.push(format!(
                "DTEND:{}",
                DateTime::from_timestamp(dtend, 0)
                    .unwrap()
                    .format("%Y%m%dT%H%M%S")
            )),
            None => (),
        };

        string_vec.push(format!("SUMMARY:{}", event.summary));
        match event.rrule {
            Some(rrule) => string_vec.push(format!(
                "RRULE:FREQ={};UNTIL={}",
                rrule.get_freq().to_string(),
                rrule.get_until().unwrap().format("%Y%m%dT%H%M%S")
            )),
            None => (),
        }
        string_vec.push("END:VEVENT".to_string());
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
        .put(config_data.basic.cal_url)
        .basic_auth(
            config_data.basic.cal_username,
            Some(config_data.basic.cal_pass),
        )
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

pub fn reschedule(blocks: Vec<TimeBlock>, task_vec: Vec<Task>, config_data: ConfigInfo) {
    let mut tasks_block = vec![];
    let mut events = vec![];
    let mut tasks = vec![];
    for block in blocks.clone() {
        if let Some(uuid_match) = task_vec.iter().find(|x| x.uuid == block.uid) {
            tasks_block.push(block.clone());
        } else {
            events.push(block);
        }
    }

    for task in tasks_block {
        let matchingtask = task_vec
            .iter()
            .find(|x| x.uuid == task.uid)
            .expect("couldnt find matching task");
        let task_from_block = Task {
            id: 0,
            estimated: task.duration.unwrap(),
            uuid: task.uid,
            description: task.summary,
            status: "pending".to_string(),
            urgency: matchingtask.urgency,
            due: matchingtask.due,
        };
        tasks.push(task_from_block);
    }
    schedule(tasks.clone(), config_data, events);
}
