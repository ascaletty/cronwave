mod config;
mod ical;
mod schedule;
mod ui;
mod whentomeet;

use inquire::Text;
use std::any::Any;

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
    // println!("\n, \n, TASKS, \n, {:?}", tasks);
    ical::fetch_ical_text(config_data.clone());
    let timeblock = ical::parse_ical_blocks();
    match args.argument.as_str() {
        "schedule" => {
            let tasks = ical::fetch_tasks();
            schedule::schedule(tasks, config_data, timeblock)
        }
        "reschedule" => {
            let tasks_scheduled = ical::fetch_tasks_scheduled();
            schedule::reschedule(timeblock, tasks_scheduled, config_data)
        }

        "done" => {
            let tasks_scheduled = ical::fetch_tasks_scheduled();
            delete(
                config_data,
                tasks_scheduled,
                args.second_arg.expect("expected task id to delete"),
                timeblock,
            )
        }
        "ui" => {
            ui::ui(timeblock);
        }
        "meet" => {
            let url = Text::new("url of when2meet").prompt().unwrap();
            let name = Text::new("name").prompt().unwrap();
            let pass = Text::new("pass").prompt().unwrap();

            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async { whentomeet::meet(url, name, pass, timeblock).await });
        }
        _ => (),
    }
}
fn delete(config_data: ConfigInfo, mut tasks: Vec<Task>, num: usize, mut blocks: Vec<TimeBlock>) {
    let task = tasks
        .iter()
        .find(|x| x.id == num)
        .expect("did not find the id number requested to delete");
    let name = task.clone().description;
    let uuid_vec: Vec<TimeBlock> = blocks
        .clone()
        .into_iter()
        .filter(|x| x.summary == name)
        .collect();
    let number = tasks
        .iter()
        .position(|x| x.id == num)
        .expect("could not find id number requested to delete");

    for uid in uuid_vec {
        let blocks_num = blocks
            .iter()
            .position(|x| x.uid == uid.uid)
            .expect("did not find the uuid in the calendar to delete");
        blocks.remove(blocks_num);
    }
    let result = std::process::Command::new("task")
        .arg("done")
        .arg(num.to_string())
        .output()
        .unwrap();
    println!("result of task delete command :{:?}", result);
    tasks.remove(number);
    schedule::reschedule(blocks, tasks, config_data);
}
