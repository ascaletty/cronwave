mod config;
mod ical;
mod schedule;
use std::{fs::File, task};

use clap::{Command, Parser};
use clap_derive::Parser as Parser_derive;
use cronwave::structs::{ConfigInfo, Task, TimeBlock};

use std::os::unix::process::CommandExt;
#[derive(Parser_derive, Debug)]
struct Args {
    argument: String,
    second_arg: Option<usize>,
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
        "delete" => delete(
            config_data,
            tasks_scheduled,
            args.second_arg.expect("expected task id to delete"),
            timeblock,
        ),
        _ => (),
    }
}
fn delete(config_data: ConfigInfo, mut tasks: Vec<Task>, num: usize, blocks: Vec<TimeBlock>) {
    let number = tasks
        .iter()
        .position(|x| x.id == num)
        .expect("did not find the number task requested");
    let result = std::process::Command::new("task")
        .arg("delete")
        .arg(num.to_string())
        .output()
        .unwrap();
    println!("result of task delete command :{:?}", result);
    tasks.remove(number);
    schedule::reschedule(blocks, tasks, config_data);
}
