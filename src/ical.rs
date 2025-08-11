use crate::ConfigInfo;
use crate::{TimeBlock, TimeBlockRaw};
use chrono::{DateTime, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use ical::property::Property;
use icalendar::{Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime};
use std::fs::read_to_string;
use std::fs::File;
use std::io::BufWriter;
use std::io::Write;
use std::os::linux::net::SocketAddrExt;
use std::sync::Mutex;

pub fn fetch_ical_text(config_data: ConfigInfo) {
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
        };
        serialized_blocks.push(data);
    }
    serialized_blocks
}
pub fn duration_to_minutes(duration: &iso8601::Duration) -> i64 {
    match duration {
        iso8601::Duration::Weeks(weeks) => *weeks as i64 * 7 * 24 * 60,
        iso8601::Duration::YMDHMS {
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond: _,
        } => {
            (*year as i64 * 365 * 24 * 60) +     // assume 365 days/year
                (*month as i64 * 30 * 24 * 60) +     // assume 30 days/month
                (*day as i64 * 24 * 60) +
                (*hour as i64 * 60) +
                (*minute as i64) +
                (*second as i64 / 60)
        }
    }
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
