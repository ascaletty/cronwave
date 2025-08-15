use std::sync::Mutex;
mod config;
mod ical;
mod schedule;

fn main() {
    let config_info = config::get_config();

    let config_data = Mutex::new(config_info.expect("failed to get config info"));
    let tasks = Mutex::new(ical::fetch_tasks());
    // println!("\n, \n, TASKS, \n, {:?}", tasks);
    ical::fetch_ical_text(
        config_data
            .lock()
            .expect("failed to unlock Mutex")
            .auth
            .clone(),
    );
    let timeblock_mutex = ical::parse_ical_blocks();
    schedule::schedule(
        tasks,
        config_data
            .lock()
            .expect("failed to unlock mutex")
            .auth
            .clone(),
        timeblock_mutex,
    );
}
