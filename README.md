# **Cronwave**
Cronwave is a calendar scheduler integration with the command line utility [taskwarrior](https://taskwarrior.org/).
Cronwave takes all your task, their estimated times and schedules them accordingly. It then links with an external caldav server and
pushes the new changes. Check out my [blog](https://aaron.scaletty.com/posts/cronwave/) post on how I use it.
## **Feature list/Roadmap**
- [x] sync with private caldav servers such as Radicale
- [x] recurring events
- [x] rescheduling feature that allows you to create new events and have your tasks move around them
- [x] delete function than completes your tasks and removes them from your calendar
- [x] tasks that start after a specific date. 
- [ ] google/apple calendar support
- [ ] ui
- [ ] more advanced ai/monte carlo simulation scheduling algorithm
- [ ] integration with [mcps](https://github.com/swaits/mcps
#### Installation
```
git clone https://github.com/ascaletty/cronwave 
cd cronwave
cargo build --release
cd target/release
sudo cp cronwave /usr/bin/
```
### Contribution
Contributions are more than welcome.









