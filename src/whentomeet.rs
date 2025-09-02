use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveTime, TimeZone};
use cronwave::structs::*;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, COOKIE, ORIGIN, REFERER, USER_AGENT};
use reqwest::Client;
use rrule::RRuleSet;
use std::collections::HashMap;
use std::error::Error;
async fn get_times(url: String) -> (Vec<String>, usize, i64, i64) {
    let html = reqwest::get(url).await.unwrap().text().await.unwrap();

    let re_times = Regex::new(r#"data-time="(\d+)""#).unwrap();
    let times: Vec<DateTime<_>> = re_times
        .captures_iter(&html)
        .map(|cap| {
            Local
                .timestamp_opt(cap[1].parse::<i64>().unwrap(), 0)
                .unwrap()
        })
        .collect();
    println!("times{:?}", times);
    let mut times_map: HashMap<NaiveDate, Vec<i64>> = HashMap::new();
    for time in times {
        let day = time.date_naive();
        times_map.entry(day).or_default().push(time.timestamp());
    }
    let first_day_blocks_num = if let Some((_day, mut dts)) = times_map.iter_mut().next() {
        dts.sort();
        let start = *dts.first().unwrap();
        let end = *dts.last().unwrap();
        ((end - start) / 900) as usize + 1
    } else {
        0
    };
    let mut days: Vec<NaiveDate> = times_map.keys().cloned().collect();
    days.sort(); // sort chronologically

    let first_day = days.first().unwrap();
    let last_day = days.last().unwrap();
    let first_day_timestamps = &times_map[first_day];
    let last_day_timestamps = &times_map[last_day];

    let start_of_first_day = *first_day_timestamps.iter().min().unwrap(); // earliest timestamp
    let end_of_last_day = *last_day_timestamps.iter().max().unwrap(); // latest timestamp

    let num_days = times_map.len();

    println!("num_days{}", num_days);
    println!("first_day_blocks_num{}", first_day_blocks_num);

    let total_blocks = first_day_blocks_num * num_days;

    let avaliablity_vec = vec!["0".to_string(); total_blocks];

    println!("avaliablity_vec{:?}", avaliablity_vec);
    println!("avaliablity_vec.len{}", avaliablity_vec.len());
    (
        avaliablity_vec,
        first_day_blocks_num,
        start_of_first_day,
        end_of_last_day,
    )
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
fn get_blocks(mut startday: i64, mut endday: i64, blocks_per_day: usize) -> Vec<Gap> {
    let mut gap_vec = vec![];
    let start = Local::timestamp_opt(&Local, startday, 0).unwrap();
    let start_day = start.day();
    if start.year() < Local::now().year() - 1 {}

    endday += 900;

    let used_secs_per_day = blocks_per_day as i64 * 900;

    let time_til_morn = 86_400 - used_secs_per_day; // leftover per day
    println!(
        "startday={}",
        Local::timestamp_opt(&Local, startday, 0).unwrap()
    );
    println!(
        "endday={}",
        Local::timestamp_opt(&Local, endday, 0).unwrap()
    );
    println!(
        "time_til_morn{}",
        Duration::seconds(time_til_morn).num_hours()
    );
    while startday < endday {
        let mut i = 0;
        while i < blocks_per_day {
            let gap = Gap {
                start: startday,
                end: startday + 900,
            };
            startday = gap.end;
            gap_vec.push(gap);
            i += 1;
        }
        //get to the start of the next day
        startday += time_til_morn;
    }
    println!("gapvec length{}", gap_vec.len());
    gap_vec
}

fn find_times_availaible(
    mut blocks: Vec<TimeBlock>,
    mut avail: Vec<String>,
    startday: i64,
    endday: i64,
    blocks_per_day: usize,
) -> String {
    let gaps = find_the_gaps(&mut blocks);
    let slots = get_blocks(startday, endday, blocks_per_day);
    let mut indexes = vec![];
    for gap in gaps.iter().filter(|x| x.start > startday) {
        let slots_avail: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(idx, slot)| slot.start >= gap.start && slot.end <= gap.end)
            .map(|(idx, _slot)| idx)
            .collect();

        indexes.append(&mut slots_avail.clone());
    }
    println!("indeces{:?}", indexes);
    for index in indexes {
        avail[index] = "1".to_string();
    }
    avail.join("").to_string()
}
pub async fn meet(
    url: String,
    name: String,
    pass: String,
    blocks: Vec<TimeBlock>,
) -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    let re = Regex::new(r"\?(.*?)\-").unwrap();
    let mut id = String::new();
    if let Some(caps) = re.captures(&url) {
        id = caps[1].to_string();
    }
    // Build headers
    let mut headers = HeaderMap::new();
    headers.insert(
        "accept",
        HeaderValue::from_static("text/javascript, text/html, application/xml, text/xml, */*"),
    );
    headers.insert(
        "accept-language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
    );
    // headers.insert("cookie", HeaderValue::from_static("_fbp=fb.1.1750035672470.609722740891632966"));
    headers.insert("dnt", HeaderValue::from_static("1"));
    headers.insert(
        ORIGIN,
        HeaderValue::from_static("https://www.when2meet.com"),
    );
    headers.insert(REFERER, HeaderValue::from_bytes(url.as_bytes()).unwrap());
    headers.insert(
        "sec-ch-ua",
        HeaderValue::from_static("\"Not?A_Brand\";v=\"99\", \"Chromium\";v=\"130\""),
    );
    headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", HeaderValue::from_static("\"Linux\""));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    headers.insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"));
    headers.insert("x-prototype-version", HeaderValue::from_static("1.7.3"));
    headers.insert(
        "x-requested-with",
        HeaderValue::from_static("XMLHttpRequest"),
    );

    // POST body
    let params = [("id", id.clone()), ("name", name), ("password", pass)];

    // Send request
    let resp = client
        .post("https://www.when2meet.com/ProcessLogin.php")
        .headers(headers)
        .form(&params)
        .send()
        .await?;

    let body = resp.text().await?;
    println!("body{}", body);

    let availiablity_info = get_times(url.clone()).await;
    let availible = find_times_availaible(
        blocks,
        availiablity_info.0,
        availiablity_info.2,
        availiablity_info.3,
        availiablity_info.1,
    );
    println!("availible{}", availible);

    post_times(id, body, availible.clone()).await
}

pub async fn post_times(
    event_id: String,
    person_id: String,
    availibility: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new();

    // Headers
    let mut headers = HeaderMap::new();
    headers.insert(
        "accept",
        HeaderValue::from_static("text/javascript, text/html, application/xml, text/xml, */*"),
    );
    headers.insert(
        "accept-language",
        HeaderValue::from_static("en-US,en;q=0.9"),
    );
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/x-www-form-urlencoded; charset=UTF-8"),
    );
    headers.insert(
        COOKIE,
        HeaderValue::from_static(
            "_fbp=fb.1.1750035672470.609722740891632966; PHPSESSID=t1629p2t01scnuon270e867h9e",
        ),
    );
    headers.insert("dnt", HeaderValue::from_static("1"));
    headers.insert(
        "origin",
        HeaderValue::from_static("https://www.when2meet.com"),
    );
    headers.insert("priority", HeaderValue::from_static("u=1, i"));
    headers.insert(
        "referer",
        HeaderValue::from_static("https://www.when2meet.com/?31840889-axNqc"),
    );
    headers.insert(
        "sec-ch-ua",
        HeaderValue::from_static(r#""Not?A_Brand";v="99", "Chromium";v="130""#),
    );
    headers.insert("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
    headers.insert("sec-ch-ua-platform", HeaderValue::from_static(r#""Linux""#));
    headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
    headers.insert("sec-fetch-mode", HeaderValue::from_static("cors"));
    headers.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
    headers.insert("user-agent", HeaderValue::from_static("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"));
    headers.insert("x-prototype-version", HeaderValue::from_static("1.7.3"));
    headers.insert(
        "x-requested-with",
        HeaderValue::from_static("XMLHttpRequest"),
    );

    let form = [
        ("person", person_id),
        ("event", event_id),
        (
            "slots",
            "1756217700,1756218600,1756219500,1756220400,1756221300".to_string(),
        ),
        ("availability", availibility),
        ("ChangeToAvailable", "true".to_string()),
    ];

    // Send request
    let res = client
        .post("https://www.when2meet.com/SaveTimes.php")
        .headers(headers)
        .form(&form)
        .send()
        .await?;

    // Debug response
    let status = res.status();
    let body = res.text().await?;
    println!("Status: {}", status);
    println!("Response body: {}", body);

    Ok(())
}
