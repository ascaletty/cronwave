mod config;
mod ical;
mod schedule;

use clap::Parser;
use clap_derive::Parser as Parser_derive;
use cronwave::structs::{ConfigInfo, Task, TimeBlock};

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
        "done" => delete(
            config_data,
            tasks_scheduled,
            args.second_arg.expect("expected task id to delete"),
            timeblock,
        ),
        _ => (),
    }
}
fn delete(config_data: ConfigInfo, mut tasks: Vec<Task>, num: usize, mut blocks: Vec<TimeBlock>) {
    let task = tasks
        .iter()
        .find(|x| x.id == num)
        .expect("did not find the id number requested to delete");
    let number = tasks
        .iter()
        .position(|x| x.id == num)
        .expect("could not find id number requested to delete");
    let uuid = task.clone().uuid;
    let blocks_num = blocks
        .iter()
        .position(|x| x.uid == uuid.clone())
        .expect("did not find the uuid in the calendar to delete");
    blocks.remove(blocks_num);

    let result = std::process::Command::new("task")
        .arg("done")
        .arg(num.to_string())
        .output()
        .unwrap();
    println!("result of task delete command :{:?}", result);
    tasks.remove(number);
    schedule::reschedule(blocks, tasks, config_data);
}
