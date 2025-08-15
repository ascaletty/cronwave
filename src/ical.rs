use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use cronwave_lib::ical::duration_to_minutes;
use cronwave_lib::structs::*;
use ical::property::Property;
use icalendar::{Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};
use std::fs::read_to_string;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::iter;
use std::os::linux::net::SocketAddrExt;
use std::process::Command;
use std::str::FromStr;
use std::sync::Mutex;

pub fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    // Input: "20250806T195556Z" → Output: DateTime<Utc>
    NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%SZ")
        .ok()
        .map(|ndt| DateTime::from_naive_utc_and_offset(ndt, Utc))
}
pub fn fetch_tasks() -> Vec<Task> {
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
fn convert_rrule(rrule: String) -> Rrule {
    let mut freq = String::new();
    let mut until_ts: i64 = 0;

    for part in rrule.split(';') {
        if part.starts_with("FREQ=") {
            freq = part["FREQ=".len()..].to_string();
        } else if part.starts_with("UNTIL=") {
            let date_str = &part["UNTIL=".len()..];
            // Parse into NaiveDateTime (no timezone yet)
            let naive_dt = NaiveDateTime::parse_from_str(date_str, "%Y%m%dT%H%M%S")
                .expect("Invalid datetime format");
            // Assume UTC and convert to Unix timestamp
            until_ts = Utc.from_utc_datetime(&naive_dt).timestamp();
        }
    }
    Rrule {
        freq,
        until: until_ts,
    }
}

pub fn fetch_ical_text(config_data: auth) {
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
        .write_all(response.text().unwrap().as_bytes())
        .expect("failed to write ics");
}
fn convert_raw_blocks(blocks: Vec<TimeBlockRaw>) -> Vec<TimeBlock> {
    let mut serialized_blocks = vec![];
    for block in blocks {
        let data = TimeBlock {
            dtstart: match block.dtstart {
                DatePerhapsTime::DateTime(start) => match start {
                    CalendarDateTime::Floating(float) => {
                        println!("found starttime is naivedatetime");
                        Local::from_local_datetime(&Local, &float).unwrap()
                    }
                    CalendarDateTime::Utc(utc) => {
                        println!("found starttime is UTC");
                        let utc_time = utc.naive_utc();
                        Local::from_utc_datetime(&Local, &utc_time)
                    }
                    CalendarDateTime::WithTimezone { date_time, tzid } => {
                        println!("found starttime is date_tiem with timezone");
                        let tz: chrono_tz::Tz = tzid.parse().unwrap();
                        Local::from_local_datetime(&Local, &date_time).unwrap()
                    }
                },
                DatePerhapsTime::Date(date) => Local
                    .from_local_datetime(&date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()))
                    .unwrap()
                    .into(),
            },
            duration: Duration::minutes(duration_to_minutes(
                &iso8601::duration(&block.duration).unwrap_or(iso8601::Duration::Weeks(0)),
            )),
            uid: block.uid,
            summary: block.summary,
            dtstamp: block.dtstamp,
            rrule: convert_rrule(block.rrule),
        };
        serialized_blocks.push(data);
    }
    serialized_blocks
}
pub fn parse_ical_blocks() -> Mutex<Vec<TimeBlock>> {
    let contents = read_to_string("school.ics").expect("couldnt read school.ics");
    let parsed_calendar: Calendar = contents.parse().unwrap();
    let mut timebloc_vec = vec![];
    for component in &parsed_calendar.components {
        if let CalendarComponent::Event(event) = component {
            let new_block = TimeBlockRaw {
                dtstart: event.get_start().unwrap(),
                duration: event
                    .properties()
                    .iter()
                    .find(|x| x.0 == "DURATION")
                    .unwrap_or((
                        &"DURATION".to_string(),
                        &icalendar::Property::new(&"DURATION".to_string(), "0"),
                    ))
                    .1
                    .value()
                    .to_string(),
                dtstamp: event.get_timestamp().unwrap(),
                summary: event.get_summary().unwrap().to_string(),
                uid: event.get_uid().unwrap().to_string(),
                rrule: event
                    .properties()
                    .iter()
                    .find(|x| x.0 == "RRULE")
                    .unwrap_or((
                        &"RRULE".to_string(),
                        &icalendar::Property::new("RRULE".to_string(), ""),
                    ))
                    .1
                    .value()
                    .to_string(),
            };
            timebloc_vec.push(new_block);
        }
    }
    println!("timebloc_vec{:?}", timebloc_vec);
    let serialized_blocks = convert_raw_blocks(timebloc_vec);
    let block_mutex: Mutex<Vec<TimeBlock>> = Mutex::new(Vec::new());
    for block in serialized_blocks {
        block_mutex.lock().unwrap().push(block);
    }
    block_mutex
}
