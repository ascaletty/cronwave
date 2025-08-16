use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use cronwave_lib::structs::{auth, ConfigInfo, Gap, Task, TimeBlock, TimeBlockRaw};
use iso8601::duration;
use reqwest::blocking::Client;
use reqwest::header::*;
use rrule::RRuleSet;
use std::os::unix::process::CommandExt;
use std::process::Command;

use std::io::Write;
use std::process::Stdio;

fn delete_all_tasks() {
    let mut child = Command::new("task")
        .arg("rc.confirmation=no")
        .arg("delete")
        .arg("all")
        .stdin(Stdio::piped())
        .spawn()
        .expect("failed to spawn task delete all");

    if let Some(stdin) = child.stdin.as_mut() {
        // Send "yes" confirmation
        stdin.write_all(b"yes\n").expect("failed to write to stdin");
    }

    let status = child.wait().expect("failed to wait on child");
    if status.success() {
        println!("All tasks deleted");
    } else {
        eprintln!("Failed to delete tasks, status: {:?}", status);
    }
}
pub fn schedule(tasks: Vec<Task>, config_data: ConfigInfo, mut blocks: Vec<TimeBlock>) {
    let mut tasks_copy = tasks.clone();
    let greatest_dur = tasks_copy.last().unwrap().estimated;

    // with_virtual.sort_by(|a, b| a.dtstart.cmp(&b.dtstart));
    let time_line = Local::now().timestamp();

    blocks.retain(|x| {
        x.dtstart + x.duration.unwrap_or(0) > time_line || x.dtend.unwrap_or(0) > time_line
    });
    let mut gaps = find_the_gaps(blocks.clone(), greatest_dur);

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

    match create_caldav_events(config_data, blocks) {
        Ok(_) => {
            println!("Events created!");
            delete_all_tasks();
        }
        Err(_) => {
            println!("events not created")
        }
    }
}
fn find_the_gaps(blocks: Vec<TimeBlock>, greatest_dur_task: i64) -> Vec<Gap> {
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

    if let Some(last) = expanded_blocks.last() {
        let horizon = last.dtstart + greatest_dur_task;
        let last_end = last.dtend.unwrap_or(last.dtstart);
        if last_end < horizon {
            gap_vec.push(Gap {
                start: last_end,
                end: horizon,
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
        .put(config_data.auth.cal_url)
        .basic_auth(
            config_data.auth.cal_username,
            Some(config_data.auth.cal_pass),
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
