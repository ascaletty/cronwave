use core::time;
use std::fmt::Debug;
use std::io;

use chrono::DateTime;
use chrono::Local;
use chrono::TimeDelta;
use chrono::Utc;
use cronwave::structs::TimeBlock;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{self, Constraint, Layout};
use ratatui::widgets::calendar::Monthly;
use ratatui::widgets::{Row, Table, TableState};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Stylize,
    symbols::border,
    text::{Line, Text},
    widgets::{Block, Cell, Paragraph, Widget},
    DefaultTerminal, Frame,
};
use ratatui::{
    style::{Color, Style},
    widgets::Borders,
};

#[derive(Debug)]
enum Focus {
    Left,
    Top,
    Bottom,
}
impl Focus {
    /// Cycle to the next pane
    fn next(&self) -> Self {
        use Focus::*;
        match self {
            Left => Top,
            Top => Bottom,
            Bottom => Left,
        }
    }

    /// Cycle to the previous pane
    fn prev(&self) -> Self {
        use Focus::*;
        match self {
            Left => Bottom,
            Top => Left,
            Bottom => Top,
        }
    }
}

#[derive(Debug)]
struct Cal {
    cols: usize,
    rows: usize,
    exit: bool,
    x: usize,
    y: usize,
    focus: Focus,
    events: Vec<TimeBlock>,
    tablestate: TableState,
}
pub fn ui(events: Vec<TimeBlock>) -> io::Result<()> {
    let mut terminal = ratatui::init();

    let app_result = Cal::default(events).run(&mut terminal);
    ratatui::restore();
    app_result
}

// impl Widget for &Cal {
//     fn render(self, area: Rect, buf: &mut Buffer) {
//         let col_constraints = (0..self.cols).map(|_| Constraint::Length(9));
//         let row_constraints = (0..self.rows).map(|_| Constraint::Length(3));
//         let horizontal = Layout::horizontal(col_constraints);
//         let vertical = Layout::vertical(row_constraints);
//
//         let rows = vertical.split(area);
//         let cells = rows.iter().flat_map(|&row| horizontal.split(row).to_vec());
//
//         for (i, cell) in cells.enumerate() {
//             Paragraph::new(format!(" {:02}", i + 1))
//                 .block(Block::bordered())
//                 .render(cell, buf);
//         }
//     }
// }
// impl Widget for Cal {
//     fn render(self, area: Rect, buf: &mut Buffer) {
//         // Draw a block around the calendar
//         let block = Block::default()
//             .title(format!("{} / {}", self.month, self.year))
//             .borders(Borders::ALL);
//         block.render(area, buf);
//
//         // Days of week header
//         let days = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
//         for (i, &day) in days.iter().enumerate() {
//             let x = area.x + 2 + (i as u16 * 3);
//             let y = area.y + 1;
//             buf.set_string(x, y, day, Style::default().fg(Color::Yellow));
//         }
//
//         // Find first day of month
//         let first = NaiveDate::from_ymd_opt(self.year, self.month, 1).unwrap();
//         let start_weekday = first.weekday().num_days_from_monday(); // 0..6
//         let days_in_month = match self.month {
//             1 => 31,
//             2 if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) => 29,
//             2 => 28,
//             3 => 31,
//             4 => 30,
//             5 => 31,
//             6 => 30,
//             7 => 31,
//             8 => 31,
//             9 => 30,
//             10 => 31,
//             11 => 30,
//             12 => 31,
//             _ => 30,
//         };
//
//         // Print days
//         let mut day = 1;
//         let mut y = area.y + 3;
//         let mut x = area.x + 2 + (start_weekday as u16 * 3);
//
//         while day <= days_in_month {
//             let date = NaiveDate::from_ymd_opt(self.year, self.month, day).unwrap();
//
//             let style = if Some(date) == self.selected {
//                 Style::default().fg(Color::Black).bg(Color::Cyan)
//             } else {
//                 Style::default()
//             };
//
//             buf.set_span(x, y, &Span::styled(format!("{:>2}", day), style), 2);
//
//             day += 1;
//             x += 3;
//             if x >= area.x + area.width - 2 {
//                 x = area.x + 2;
//                 y += 2;
//             }
//         }
//     }
// }
fn focused_block<'a>(title: &'a str, is_focused: bool) -> Block<'a> {
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow) // highlighted
    } else {
        Style::default().fg(Color::White) // normal
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style)
}
impl Cal {
    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.handle_events()?;
        }
        Ok(())
    }
    pub fn draw(&mut self, frame: &mut Frame) {
        let [base, bar] = Layout::vertical([Constraint::Percentage(97), Constraint::Percentage(3)])
            .areas(frame.area());
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
                .areas(base);
        let [top_right, bottom_right] = Layout::vertical([Constraint::Fill(1); 2]).areas(right);
        let p = Line::raw("q<quit> j<down> k<up>");

        let left_b = focused_block("left", matches!(self.focus, Focus::Left));
        let top_b = focused_block("top", matches!(self.focus, Focus::Top));
        let brendan = focused_block("bottom", matches!(self.focus, Focus::Bottom));
        let mut rows = Vec::new();
        let header = Row::new(vec!["Summary", "Start", "End"]);
        let timeline = Local::now().timestamp();
        for event in &self.events {
            let time = DateTime::from_timestamp(event.dtstart, 0).unwrap();

            rows.push(Row::new(vec![
                event.summary.clone(),
                chrono::DateTime::from_timestamp(event.dtstart, 0)
                    .unwrap()
                    .to_string(),
                chrono::DateTime::from_timestamp(event.dtstart + event.duration.unwrap(), 0)
                    .unwrap()
                    .to_string(),
            ]));
        }

        let widths = [
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ];

        let table = Table::new(rows, widths)
            .block(left_b)
            .header(header)
            .highlight_symbol(">>");
        frame.render_widget(top_b, top_right);
        frame.render_widget(p, bar);
        frame.render_stateful_widget(table, left, &mut self.tablestate);
        frame.render_widget(brendan, bottom_right);
    }

    pub fn handle_key_event(&mut self, key: KeyEvent) {
        match self.focus {
            Focus::Left => {
                match key.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Tab => self.focus = Focus::next(&self.focus),
                    KeyCode::BackTab => self.focus = Focus::prev(&self.focus),
                    KeyCode::Char('j') => {
                        // print!("");
                        let i = match self.tablestate.selected() {
                            Some(i) => {
                                if i >= self.events.len() - 1 {
                                    0
                                } else {
                                    i + 1
                                }
                            }
                            None => 0,
                        };
                        self.tablestate.select(Some(i));
                    }
                    KeyCode::Char('k') => {
                        // print!("up pressed");
                        let i = match self.tablestate.selected() {
                            Some(i) => {
                                if i == 0 {
                                    self.events.len() - 1
                                } else {
                                    i - 1
                                }
                            }
                            None => 0,
                        };
                        self.tablestate.select(Some(i));
                    }
                    _ => {}
                }
            }
            Focus::Top => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::Tab => self.focus = Focus::next(&self.focus),
                KeyCode::BackTab => self.focus = Focus::prev(&self.focus),
                _ => {}
            },
            Focus::Bottom => match key.code {
                KeyCode::Char('q') => self.exit = true,
                KeyCode::Tab => self.focus = Focus::next(&self.focus),
                KeyCode::BackTab => self.focus = Focus::prev(&self.focus),
                _ => {}
            },
        }
    }
    pub fn default(events: Vec<TimeBlock>) -> Self {
        Self {
            cols: 7,
            rows: 5,
            exit: false,
            x: 0,
            y: 0,
            focus: Focus::Left,
            events,
            tablestate: TableState::default().with_selected(0),
        }
    }
    pub fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }
}
