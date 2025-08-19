use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use cronwave::structs::*;
use ical::property::Property;
use icalendar::{Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};
use iso8601_duration::Duration;
use std::fs::read_to_string;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::process::Command;
use std::str::FromStr;
fn week_and_day(week: u32, day: u32, year: i32) -> i64 {
    let start = Local::with_ymd_and_hms(&Local, year, 0, 0, 0, 0, 0)
        .unwrap()
        .timestamp();
    let num_days = week * 7 + day;
    let num_day_i = num_days as i64;
    num_day_i * 864000 + start
}
fn ordinal(year: i32, ddd: u32) -> i64 {
    let start = Local::with_ymd_and_hms(&Local, year, 0, 0, 0, 0, 0)
        .unwrap()
        .timestamp();
    let num_days = ddd as i64;
    num_days * 864000 + start
}
fn convert_iso8601_to_timestamp(dur: iso8601::DateTime) -> i64 {
    match dur.date {
        iso8601::Date::YMD { year, month, day } => Local::with_ymd_and_hms(
            &Local,
            year,
            month,
            day,
            dur.time.hour,
            dur.time.minute,
            dur.time.second,
        )
        .unwrap()
        .timestamp(),
        iso8601::Date::Week {
            year: year,
            ww: ww,
            d: d,
        } => week_and_day(ww, d, year),
        iso8601::Date::Ordinal { year: y, ddd: ddd } => ordinal(y, ddd),
    }
}
fn iso8601_dur_to_timestamp(dur: iso8601::Duration) -> i64 {
    match dur {
        iso8601::Duration::YMDHMS {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
        } => {
            (year * 31536000 + month * 2629746 + day * 86400 + hour * 3600 + minute * 60 + second)
                as i64
        }

        iso8601::Duration::Weeks(week) => week_and_day(week, 0, 0),
    }
}

pub fn fetch_tasks() -> Vec<Task> {
    let task_command = Command::new("task")
        .args(["status:pending", "+unscheduled", "export"])
        .output()
        .expect("failed to run task export");
    let mut task_uuid_vec = vec![];

    let uuids = File::create("uuids.txt").unwrap();
    let mut writer = BufWriter::new(uuids);

    let output_raw: Vec<RawTask> =
        serde_json::from_slice(&task_command.stdout).expect("invalid taskwarrior output");
    let mut output = vec![];
    for task in output_raw {
        let task_item = Task {
            uuid: task.uuid,
            description: task.description,
            due: convert_iso8601_to_timestamp(iso8601::DateTime::from_str(&task.due).unwrap()),
            estimated: iso8601_dur_to_timestamp(
                iso8601::Duration::from_str(&task.estimated).unwrap(),
            ),
            status: task.status,
            urgency: task.urgency,
        };

        task_uuid_vec.push(task_item.clone().uuid);
        output.push(task_item);
    }
    writer
        .write_all(task_uuid_vec.join(";").as_bytes())
        .expect("failed to write ics");

    output.sort_by(|a, b| a.due.cmp(&b.due));
    // println!("TASKS OUTPUT");
    // println!("output of fetch tasks{:?}", output);

    output
}

pub fn fetch_tasks_scheduled() -> Vec<Task> {
    let task_command = Command::new("task")
        .args(["status:pending", "+scheduled", "export"])
        .output()
        .expect("failed to run task export");
    let mut task_uuid_vec = vec![];

    let uuids = File::create("uuids.txt").unwrap();
    let mut writer = BufWriter::new(uuids);

    let output_raw: Vec<RawTask> =
        serde_json::from_slice(&task_command.stdout).expect("invalid taskwarrior output");
    let mut output = vec![];
    for task in output_raw {
        let task_item = Task {
            uuid: task.uuid,
            description: task.description,
            due: convert_iso8601_to_timestamp(iso8601::DateTime::from_str(&task.due).unwrap()),
            estimated: iso8601_dur_to_timestamp(
                iso8601::Duration::from_str(&task.estimated).unwrap(),
            ),
            status: task.status,
            urgency: task.urgency,
        };

        task_uuid_vec.push(task_item.clone().uuid);
        output.push(task_item);
    }
    writer
        .write_all(task_uuid_vec.join(";").as_bytes())
        .expect("failed to write ics");

    output.sort_by(|a, b| a.due.cmp(&b.due));
    // println!("TASKS OUTPUT");
    // println!("output of fetch tasks{:?}", output);

    output
}
pub fn fetch_ical_text(config_data: ConfigInfo) {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(config_data.auth.cal_url)
        .basic_auth(
            config_data.auth.cal_username,
            Some(config_data.auth.cal_pass),
        )
        .send()
        .expect("failed to fetch ical");
    let school = File::create("school.ics").unwrap();
    let mut writer = BufWriter::new(school);
    // println!("Writen the ics{:?}", response.text());
    writer
        .write_all(response.text().unwrap().as_bytes())
        .expect("failed to write ics");
}
pub fn parse_ical_blocks<'b>() -> Vec<TimeBlock> {
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
                Some(dur) => {
                    let float = dur
                        .value()
                        .parse::<Duration>()
                        .unwrap()
                        .num_seconds()
                        .unwrap();
                    Some(float as i64)
                }
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
    println!("print blocks{:?}", timebloc_vec);
    timebloc_vec
}
