mod config;
mod ical;
mod schedule;
use std::task;

use clap::Parser;
use clap_derive::Parser as Parser_derive;
use cronwave::structs::{ConfigInfo, Task, TimeBlock};
#[derive(Parser_derive, Debug)]
struct Args {
    argument: String,
}

fn main() {
    let args = Args::try_parse().unwrap();

    let config_info = config::get_config();

    let config_data = config_info.expect("failed to get config info");
    let tasks = ical::fetch_tasks();
    let tasks_scheduled = ical::fetch_tasks_scheduled();
    // println!("\n, \n, TASKS, \n, {:?}", tasks);
    ical::fetch_ical_text(config_data.clone());
    let timeblock = ical::parse_ical_blocks();
    match args.argument.as_str() {
        "schedule" => schedule::schedule(tasks, config_data, timeblock),
        "reschedule" => schedule::reschedule(timeblock, tasks_scheduled, config_data),
        _ => (),
    }
}
