use std::{
    collections::HashMap,
    error::Error,
    process::ExitStatus,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
};

mod iterm;

mod config;

use sysinfo::Pid;

mod tabadapter;

mod tmux;

use ratatui::{
    CompletedFrame, DefaultTerminal, Frame,
    layout::{Constraint, Flex, Layout},
    style::Style,
    text::Text,
    widgets::{Block, Row, Table, Widget},
};
use std::sync::mpsc::channel;
use std::thread;
use tmux_interface::Formats;
use yaml_rust2::yaml::Hash;

use crate::{
    config::try_load_config,
    iterm::ITermTabAdapter,
    tabadapter::TabAdapter,
    tmux::{RunningProgram, StartedProgram, cleanup_session, convert_pids, start_command},
};

enum AppStatus {
    Started,
    Running,
    Dead,
}

struct DisplayStatus {
    app_statuses: HashMap<String, AppStatus>,
    outstanding_pids: Vec<Pid>,
    dead_sessions: Vec<String>,
    join_handles: Vec<JoinHandle<()>>,
}

impl DisplayStatus {
    fn new() -> Self {
        DisplayStatus {
            app_statuses: HashMap::new(),
            outstanding_pids: Vec::new(),
            dead_sessions: Vec::new(),
            join_handles: Vec::new(),
        }
    }

    fn mark_app_started(&mut self, app_name: &str) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Started);
    }

    fn mark_app_running(&mut self, app_name: &str) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Running);
    }

    fn mark_app_dead(&mut self, app_name: &str) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Dead);
    }

    fn enqueue_receiver(&mut self, recv: JoinHandle<()>) {
        self.join_handles.push(recv);
    }
}

impl std::fmt::Display for AppStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dead => f.write_str("X"),
            Self::Running => f.write_str("R"),
            Self::Started => f.write_str("S"),
        }
    }
}

impl Widget for &DisplayStatus {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let mut rows = Vec::new();
        for (aname, astatus) in self.app_statuses.iter() {
            let row = Row::from_iter(vec![aname.to_owned(), format!("{}", astatus)]);
            rows.push(row);
        }
        let widths = vec![Constraint::Length(45), Constraint::Length(1)];
        let table = Table::new(rows, widths);
        let vlayout = Layout::vertical(vec![Constraint::Length(self.app_statuses.len() as u16)])
            .flex(Flex::Center);
        let hlayout = Layout::horizontal(vec![Constraint::Length(46)]).flex(Flex::Center);
        let [area] = hlayout.areas(area);
        let [area] = vlayout.areas(area);
        table.render(area, buf);
    }
}

#[derive(Debug, Clone)]
enum TmuxProcessOutcome {
    ReceiveErr,
    ProcessEnded(String, Pid, Pid, Option<ExitStatus>),
}

fn choose_tab_adapter() -> Result<Option<impl TabAdapter>, Box<dyn Error>> {
    let ta = ITermTabAdapter::new()?;
    Ok(Some(ta))
}

fn wait_for_term(
    out_chan: &Sender<TmuxProcessOutcome>,
    running_p: &RunningProgram,
) -> JoinHandle<()> {
    let rp = (*running_p).clone();
    let tx = out_chan.clone();
    thread::spawn(move || {
        let s: sysinfo::System = sysinfo::System::new_all();
        let p_proc = s.process(rp.program.program_pid);
        if let Some(_p_pid) = p_proc {
            let stat = p_proc.unwrap().wait();
            let _ = tx.send(TmuxProcessOutcome::ProcessEnded(
                rp.program.session_name,
                rp.program.tmux_pid,
                rp.program.program_pid,
                stat,
            ));
        } else {
            let _ = tx.send(TmuxProcessOutcome::ProcessEnded(
                rp.program.session_name,
                rp.program.tmux_pid,
                rp.program.program_pid,
                None,
            ));
        }
    })
}

fn check_for_message(
    rx: &Receiver<TmuxProcessOutcome>,
    outstanding_pids: &Vec<Pid>,
) -> Option<TmuxProcessOutcome> {
    if outstanding_pids.is_empty() {
        return None;
    }
    if let Ok(msg) = rx.recv_timeout(std::time::Duration::from_millis(29)) {
        Some(msg)
    } else {
        Some(TmuxProcessOutcome::ReceiveErr)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();

    let exe_loc = std::env::current_dir().unwrap();
    let exe_path = exe_loc.canonicalize().unwrap();

    let config = try_load_config(&exe_path, &mut args)?;

    let mut started_commands: Vec<StartedProgram> = Vec::new();
    let mut display_status = DisplayStatus::new();
    for spec in config.apps.iter() {
        let comm = start_command(&config.namespace, &spec)?;
        started_commands.push(comm);
        display_status.mark_app_started(&spec.name);
    }
    let mut running_programs = convert_pids(&started_commands)?;
    let mut tab_adapter = choose_tab_adapter()?;
    if let Some(ta) = tab_adapter.as_mut() {
        for c in running_programs.iter_mut() {
            ta.open(&c.program.session_name);
        }
        ta.after_all_open();
    }
    let (tx, rx) = channel::<TmuxProcessOutcome>();
    let mut outstanding_pids = Vec::new();
    let mut dead_sessions = Vec::new();
    for c in running_programs.iter() {
        outstanding_pids.push(c.program.program_pid);
        display_status.mark_app_running(&c.spec.name);
        display_status.enqueue_receiver(wait_for_term(&tx, &c));
    }
    let mut terminal = ratatui::init();
    while let Some(evt) = check_for_message(&rx, &outstanding_pids) {
        match evt {
            TmuxProcessOutcome::ProcessEnded(s, _t_pid, p_pid, _) => {
                outstanding_pids.retain(|f| f != &p_pid);
                display_status.mark_app_dead(&s);
                dead_sessions.push(s);
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
            _ => {
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
        }
    }
    for ds in dead_sessions.iter() {
        cleanup_session(ds);
        if let Some(ta) = tab_adapter.as_mut() {
            ta.close(ds);
        }
    }
    if let Some(ta) = tab_adapter.as_mut() {
        ta.after_all_closed();
    }
    ratatui::restore();
    Ok(())
}
