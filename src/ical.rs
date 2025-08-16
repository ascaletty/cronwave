use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use cronwave_lib::structs::*;
use icalendar::{Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};
use iso8601_duration::Duration;
use std::fs::read_to_string;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::process::Command;
use std::str::FromStr;

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
            due: DateTime::from_str(&task.due).unwrap().timestamp(),
            estimated: DateTime::from_str(&task.estimated).unwrap().timestamp(),
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
pub fn parse_ical_blocks() -> Vec<TimeBlock> {
    let contents = read_to_string("school.ics").expect("couldnt read school.ics");
    let parsed_calendar: Calendar = contents.parse().unwrap();
    let mut timebloc_vec = vec![];
    for component in &parsed_calendar.components {
        if let CalendarComponent::Event(event) = component {
            let dtstart = match event.get_start() {
                Some(dt) => match dt {
                    DatePerhapsTime::Date(date) => Local
                        .from_local_datetime(
                            &date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
                        )
                        .unwrap()
                        .timestamp(),
                    DatePerhapsTime::DateTime(dt) => match dt {
                        CalendarDateTime::Floating(float) => {
                            Local::from_local_datetime(&Local, &float)
                                .unwrap()
                                .timestamp()
                        }
                        CalendarDateTime::Utc(utc) => {
                            Local::from_utc_datetime(&Local, &utc.naive_utc()).timestamp()
                        }
                        CalendarDateTime::WithTimezone { date_time, tzid } => {
                            Local::from_local_datetime(&Local, &date_time)
                                .unwrap()
                                .timestamp()
                        }
                    },
                },
                None => panic!("no event start"),
            };
            let duration = match event.properties().get("DURATION") {
                Some(dur) => Some(dur.value().parse::<Duration>().unwrap().num_seconds()),
                None => None,
            };
            let uid = event.get_uid().expect("no uuid");
            let summary = event.get_summary().expect("no summary");
            let dtstamp = event.get_timestamp().unwrap();
            let rrule = match event.properties().get("RRULE") {
                Some(rrule) => {
                    let rrule =
                        rrule::RRule::from_str(rrule.value()).expect("failed to parse to rrule");
                    let tz = rrule::Tz::Local(Local);
                    let time = Utc::timestamp_opt(&Utc, dtstart, 0)
                        .unwrap()
                        .with_timezone(&tz);
                    Some(rrule.validate(time).unwrap())
                }
                None => None,
            };
            let dtend = match event.get_end() {
                Some(end) => match end {
                    DatePerhapsTime::Date(date) => Some(
                        Local
                            .from_local_datetime(
                                &date.and_time(NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
                            )
                            .unwrap()
                            .timestamp(),
                    ),
                    DatePerhapsTime::DateTime(dt) => match dt {
                        CalendarDateTime::Floating(float) => Some(
                            Local::from_local_datetime(&Local, &float)
                                .unwrap()
                                .timestamp(),
                        ),
                        CalendarDateTime::Utc(utc) => {
                            Some(Local::from_utc_datetime(&Local, &utc.naive_utc()).timestamp())
                        }
                        CalendarDateTime::WithTimezone { date_time, tzid } => {
                            let tz: chrono_tz::Tz = tzid.parse().unwrap();
                            Some(
                                Local::from_local_datetime(&Local, &date_time)
                                    .unwrap()
                                    .timestamp(),
                            )
                        }
                    },
                },
                None => None,
            };
            timebloc_vec.push(TimeBlock {
                dtstart,
                dtend,
                summary: summary.to_string(),
                rrule,
                uid: uid.to_string(),
                duration,
                dtstamp,
            });
        }
    }

    timebloc_vec
}
