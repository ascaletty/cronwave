use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use cronwave_lib::structs::{auth, ConfigInfo, Gap, Task, TimeBlock, TimeBlockRaw};
use reqwest::blocking::Client;
use reqwest::header::*;
use rrule::RRuleSet;
use std::os::unix::process::CommandExt;
use std::process::Command;
fn create_virtual_events(blocks: Vec<TimeBlock>) -> Vec<TimeBlock> {
    let current_time = Local::now().timestamp();
    let mut with_virtual = vec![];
    for block in blocks {
        let rruleset = RRuleSet::new(DateTime::from_timestamp(block.dtstart, 0).unwrap())
            .rrule(block.rrule.unwrap());
        let result = rruleset.all_unchecked();
        for time in result {
            let new_block = TimeBlock {
                dtstart: time.timestamp(),
                duration: block.duration,
                dtstamp: block.dtstamp,
                dtend: None,
                summary: block.summary,
                uid: block.uid,
                rrule: None,
            };
            with_virtual.push(new_block);
        }
    }
    with_virtual
}
pub fn schedule(tasks: Vec<Task>, config_data: auth, mut blocks: Vec<TimeBlock>) {
    let mut tasks_copy = tasks.clone();
    let greatest_dur = tasks_copy.last().unwrap().estimated;
    let mut repeating_clone = blocks.clone();
    repeating_clone.retain(|x| x.rrule != None);
    let mut with_virtual = create_virtual_events(repeating_clone);

    with_virtual.sort_by(|a, b| a.dtstart.cmp(&b.dtstart));
    // println!("block gaurs \n \n \n {:?}", block_guard);

    let time_line = Local::now().timestamp();

    let mut blocks_copy = with_virtual.clone();

    blocks_copy.retain(|x| x.dtstart + x.duration > time_line);
    let mut gaps = find_the_gaps(blocks_copy, greatest_dur);

    let mut scheduled = vec![];
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
            scheduled.push(TimeBlock {
                duration: task.estimated,
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
    for block in scheduled {
        blocks.push(block);
    }
    let last_time_scheduled = blocks.last().unwrap().dtstart;
    let mut time_line_after = last_time_scheduled;
    for task in tasks_copy {
        if time_line_after > task.due {
            println!("task will not be completed in time");
        }
        blocks.push(TimeBlock {
            duration: task.estimated,
            dtstart: time_line_after,
            dtend: None,
            uid: task.uuid,
            dtstamp: Utc::now(),
            summary: task.description.clone(),
            rrule: None,
        });
        //update time_line_after
        time_line_after += task.estimated
    }

    // block_guard.sort_by(|a, b| a.dtstart.timestamp().cmp(&b.dtstart.timestamp()));
    // println!("{:?}", block_guard);
    match create_caldav_events(config_data, blocks) {
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
        .dtstart;
    let mut gap_vec = vec![];
    let mut i = 0;
    for block in blocks.clone() {
        i += 1;
        let gap = Gap {
            start: block.dtstart + block.duration,
            end: blocks
                .get(i)
                .unwrap_or(&TimeBlock::last(last_block, greatest_dur_task))
                .dtstart,
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
    blocks: Vec<TimeBlock>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut string_vec = vec!["BEGIN:VCALENDAR".to_string()];

    for event in blocks {
        string_vec.push("BEGIN:VEVENT".to_string());
        string_vec.push(format!("UID:{}", event.uid));
        string_vec.push(format!("DTSTAMP:{}", event.dtstamp));
        match event.duration {
            Some(dur) => string_vec.push(format!(
                "DURATION:{}",
                DateTime::from_timestamp(dur, 0)
                    .unwrap()
                    .format("%Y%m%dt%H%M%S")
            )),
            None => (),
        };
        match event.dtend {
            Some(dtend) => string_vec.push(format!("DTEND:{}", DateTime::from_timestamp(dtend, 0))),
            None => (),
        };

        string_vec.push(format!("SUMMARY:{}", event.summary));
        match event.rrule {
            Some(rrule) => string_vec.push(format!("RRULE:{}", rrule.to_string())),
            None => (),
        }
        string_vec.push("END:VEVENT");
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
