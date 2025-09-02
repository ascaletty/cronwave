use chrono::DateTime;
use chrono::Local;
use chrono::TimeZone;
use chrono::Utc;
use color_eyre::owo_colors::colors::xterm;
use cronwave::structs::*;
use reqwest::blocking::Client;
use reqwest::header::*;
use rrule::RRuleSet;
use std::process::Command;
use std::thread::current;

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
        println!("result={}", result);
    }
}

pub fn schedule(mut tasks: Vec<Task>, config_data: ConfigInfo, mut blocks: Vec<TimeBlock>) {
    let time_line = Local::now().timestamp();

    blocks.retain(|x| {
        x.dtstart + x.duration.unwrap_or(0) > time_line
            || x.dtend.unwrap_or(0) > time_line
            || x.rrule.is_some()
    });
    let mut gaps = find_the_gaps(&mut blocks);

    gaps.retain(|x| x.end > time_line);
    for gap in gaps {
        let mut start = gap.start;
        let mut time_til = gap.end - start;
        //need to sort tasks by due date still
        tasks.sort_by_key(|x| x.due);
        while let Some((idx, task)) = tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.status != "scheduled".to_string()
                    && (t.start.is_none() || start > t.start.unwrap())
            })
            .min_by_key(|(_, t)| t.due)
        {
            if time_til == 0 {
                break;
            }
            if task.estimated > time_til {
                blocks.push(TimeBlock {
                    duration: Some(time_til),
                    dtstart: start,
                    dtend: None,
                    rrule: None,
                    uid: task.uuid.clone(),
                    summary: task.description.clone(),
                    dtstamp: Utc::now(),
                });
                tasks.push(Task {
                    description: task.description.clone(),
                    estimated: task.estimated - time_til,
                    id: task.id,
                    uuid: uuid::Uuid::new_v4().to_string(),
                    due: task.due,
                    status: "unscheduled".to_string(),
                    urgency: task.urgency,
                    start: task.start,
                });
                start += time_til;
                time_til = 0;
                tasks[idx].status = "scheduled".to_string();
            } else {
                blocks.push(TimeBlock {
                    duration: Some(task.estimated),
                    dtstart: start,
                    summary: task.description.clone(),
                    dtend: None,
                    dtstamp: Utc::now(),
                    rrule: None,
                    uid: task.uuid.clone(),
                });
                start += task.estimated;
                time_til -= task.estimated;
                tasks[idx].status = "scheduled".to_string();
            }
        }
    }
    // let last_block = blocks.last().unwrap();
    // let last_time_scheduled = match last_block.duration {
    //     Some(dur) => last_block.dtstart + dur,
    //     None => last_block.dtend.unwrap(),
    // };
    // let mut time_line_after = last_time_scheduled;
    //
    // tasks.retain(|x| x.status == "pending".to_string());
    // for task in tasks.as_slice() {
    //     if time_line_after > task.due {
    //         println!("task will not be completed in time");
    //     }
    //     blocks.push(TimeBlock {
    //         duration: Some(task.estimated),
    //         dtstart: time_line_after,
    //         dtend: None,
    //         rrule: None,
    //         uid: task.uuid.clone(),
    //         dtstamp: Utc::now(),
    //         summary: task.description.clone(),
    //     });
    //     //update time_line_after
    //     time_line_after += task.estimated
    // }

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

fn split_gap_into_days(mut start: i64, end: i64, gaps: &mut Vec<Gap>) {
    while start < end {
        let start_dt = Local.timestamp_opt(start, 0).unwrap();
        let next_day_start = start_dt
            .date_naive()
            .succ_opt()
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let next_day_ts = Local
            .from_local_datetime(&next_day_start)
            .unwrap()
            .timestamp();

        let chunk_end = std::cmp::min(end, next_day_ts);

        gaps.push(Gap {
            start,
            end: chunk_end,
        });

        start = chunk_end;
    }
}

fn find_the_gaps(blocks: &mut Vec<TimeBlock>) -> Vec<Gap> {
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
            expanded_blocks.push(block.clone());
        }
    }
    expanded_blocks.sort_by_key(|b| b.dtstart);

    let now = Local::now().timestamp();
    println!("time is {}", Local::now());
    expanded_blocks.retain(|x| {
        let end = match x.duration {
            Some(dur) => dur + x.dtstart,
            None => x.dtend.unwrap(),
        };
        end > now
    });
    expanded_blocks.first_mut().unwrap().dtstart = now;
    println!("\nexpanded_blocks{:?}\n", expanded_blocks);

    // Walk through and find gaps between consecutive blocks
    for w in expanded_blocks.windows(2) {
        let current = &w[0];
        let next = &w[1];
        let current_end = match current.duration {
            Some(dur) => current.dtstart + dur,
            None => current.dtend.unwrap(),
        };
        println!(
            "block:{}, block_end:{}, next start:{}",
            current.summary,
            Local::timestamp_opt(&Local, current_end, 0).unwrap(),
            Local::timestamp_opt(&Local, next.dtstart, 0).unwrap()
        );

        if current_end < next.dtstart && current_end > now {
            split_gap_into_days(current_end, next.dtstart, &mut gap_vec);
        }
    }
    for gap in gap_vec.clone() {
        println!(
            "gap from {} to {}",
            Local::timestamp_opt(&Local, gap.start, 0).unwrap(),
            Local::timestamp_opt(&Local, gap.end, 0).unwrap()
        );
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
        let dur = event.duration;
        if dur.is_some() {
            let hours = chrono::Duration::seconds(dur.unwrap()).num_hours() * 3600;
            let minutes = chrono::Duration::seconds(dur.unwrap() - hours).num_minutes();
            if hours == 0 {
                string_vec.push(format!(
                    "DURATION:PT{}M",
                    chrono::Duration::seconds(dur.unwrap()).num_minutes()
                ));
            } else if minutes == 0 {
                string_vec.push(format!(
                    "DURATION:PT{}H",
                    chrono::Duration::seconds(dur.unwrap()).num_hours()
                ));
            } else {
                string_vec.push(format!(
                    "DURATION:PT{}H{}M",
                    chrono::Duration::seconds(dur.unwrap()).num_hours(),
                    chrono::Duration::seconds(dur.unwrap() - hours).num_minutes()
                ));
            }
        }
        if event.dtend.is_some() {
            let dtend = event.dtend.unwrap();
            string_vec.push(format!(
                "DTEND:{}",
                DateTime::from_timestamp(dtend, 0)
                    .unwrap()
                    .format("%Y%m%dT%H%M%S")
            ));
        };
        string_vec.push(format!("SUMMARY:{}", event.summary));
        if event.rrule.is_some() {
            let rrule = event.rrule.unwrap();
            string_vec.push(format!(
                "RRULE:FREQ={};UNTIL={}",
                rrule.get_freq(),
                rrule.get_until().unwrap().format("%Y%m%dT%H%M%S")
            ));
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

    // Send the PUT request with Basic auth
    let response = client
        .put(config_data.Basic.cal_url)
        .basic_auth(
            config_data.Basic.cal_username,
            Some(config_data.Basic.cal_pass),
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

pub fn reschedule(blocks: Vec<TimeBlock>, mut task_vec: Vec<Task>, config_data: ConfigInfo) {
    let mut tasks_block = vec![];

    let mut events = vec![];
    for block in blocks.clone() {
        if let Some(_uuid_match) = task_vec.iter().find(|x| x.description == block.summary) {
            tasks_block.push(block);
        } else {
            events.push(block);
        }
    }
    for task in tasks_block.as_slice() {
        if let Some(existing) = task_vec.iter_mut().find(|x| x.description == task.summary) {
            existing.start = Some(task.dtstart);
            existing.estimated = task.duration.unwrap();
        }
    }

    print!("timeblocks found from tasks{:?}", tasks_block);
    schedule(task_vec, config_data, events);
}
