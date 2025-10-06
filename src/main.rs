use std::{
    collections::HashMap,
    error::Error,
    process::ExitStatus,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
    time::Duration,
};

mod config;

use sysinfo::Pid;

mod tabadapter;

mod tmux;

mod processes;

use ratatui::{
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Flex, Layout},
    widgets::{Row, Table, Widget},
};
use std::sync::mpsc::channel;
use std::thread;

use crate::{
    config::try_load_config,
    processes::kill_process,
    tabadapter::{TabAdapter, choose_tab_adapter},
    tmux::{RunningProgram, StartedProgram, cleanup_session, convert_pids, start_command},
};

enum AppStatus {
    Started,
    Running,
    Dead,
}

struct DisplayStatus {
    app_statuses: HashMap<String, AppStatus>,
    pid_map: HashMap<Pid, String>,
    outstanding_pids: Vec<Pid>,
    dead_sessions: Vec<String>,
    join_handles: Vec<JoinHandle<()>>,
    event_handle: Option<JoinHandle<()>>,
    event_signal_channel: Option<Sender<()>>,
    is_quiting: bool,
    killer_procs: Option<Vec<JoinHandle<()>>>,
    tab_adapter: Option<Box<dyn TabAdapter>>,
    child_event_listener: Receiver<AppEvent>,
    child_event_sender: Sender<AppEvent>,
}

impl DisplayStatus {
    fn new(ta: Option<Box<dyn TabAdapter>>) -> Self {
        let (ces, cel) = channel::<AppEvent>();
        DisplayStatus {
            app_statuses: HashMap::new(),
            outstanding_pids: Vec::new(),
            pid_map: HashMap::new(),
            dead_sessions: Vec::new(),
            join_handles: Vec::new(),
            event_handle: None,
            event_signal_channel: None,
            is_quiting: false,
            killer_procs: None,
            tab_adapter: ta,
            child_event_listener: cel,
            child_event_sender: ces,
        }
    }

    fn mark_app_started(&mut self, app_name: &str) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Started);
    }

    fn mark_app_running(&mut self, app_name: &str, session_name: &str, pid: &Pid) {
        self.outstanding_pids.push(pid.clone());
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Running);
        self.pid_map.insert(pid.clone(), session_name.to_owned());
    }

    fn mark_app_dead(&mut self, app_name: &str, session_name: &str, pid: &Pid) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Dead);
        self.outstanding_pids.retain(|f| f != pid);
        self.dead_sessions.push(session_name.to_owned());
    }

    fn enqueue_receiver(&mut self, recv: JoinHandle<()>) {
        self.join_handles.push(recv);
    }

    fn wait_for_handles(&mut self) {
        while !self.join_handles.is_empty() {
            let item = self.join_handles.pop();
            let _ = item.unwrap().join();
        }
    }

    fn start_running(&mut self, running_programs: &Vec<RunningProgram>) {
        let (es, dc) = channel::<()>();
        if let Some(ta) = self.tab_adapter.as_mut() {
            for c in running_programs.iter() {
                ta.open(&c.program.session_name);
            }
            ta.after_all_open();
        }
        for c in running_programs.iter() {
            self.mark_app_running(
                &c.spec.name,
                &c.program.session_name,
                &c.program.program_pid,
            );
            self.enqueue_receiver(wait_for_term(&self.child_event_sender, &c));
        }
        self.event_signal_channel = Some(es);
        self.event_handle = Some(start_event_loop(&self.child_event_sender, dc));
    }

    fn finish_running_with_adapter(&mut self) {
        if let Some(ta) = self.tab_adapter.as_mut() {
            ta.after_all_closed();
        }
    }

    fn shutdown_session(&mut self, session_name: &str) {
        cleanup_session(session_name);
        if let Some(ta) = self.tab_adapter.as_mut() {
            ta.close(session_name);
        }
    }

    fn shut_down_events(self) {
        if let Some(esc) = self.event_signal_channel {
            let _ = esc.send(());
        }
        if let Some(eh) = self.event_handle {
            let _ = eh.join();
        }
        if let Some(mut kp) = self.killer_procs {
            while !kp.is_empty() {
                if let Some(kp_jh) = kp.pop() {
                    let _ = kp_jh.join();
                }
            }
        }
    }

    fn execute_quit(&mut self) {
        if !self.is_quiting {
            self.is_quiting = true;
            let mut kps = Vec::new();
            for p in self.outstanding_pids.iter() {
                let the_process = p.clone();
                let session_name = self.pid_map.get(&the_process);
                let owned_sn = session_name.map(|s| s.to_owned());
                kps.push(thread::spawn(move || {
                    kill_process(&the_process, &owned_sn);
                }));
            }
            self.killer_procs = Some(kps);
        }
    }

    fn finish_shutdown(mut self) {
        for sn in self.dead_sessions.clone().iter() {
            self.shutdown_session(&sn);
        }
        self.finish_running_with_adapter();
        self.wait_for_handles();
        self.shut_down_events();
    }
}

impl std::fmt::Display for AppStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dead => f.write_str("❌"),
            Self::Running => f.write_str("🚀"),
            Self::Started => f.write_str("🛫"),
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
        let widths = vec![Constraint::Length(46), Constraint::Length(4)];
        let table = Table::new(rows, widths);
        let vlayout = Layout::vertical(vec![Constraint::Length(self.app_statuses.len() as u16)])
            .flex(Flex::Center);
        let hlayout = Layout::horizontal(vec![Constraint::Length(50)]).flex(Flex::Center);
        let [area] = hlayout.areas(area);
        let [area] = vlayout.areas(area);
        table.render(area, buf);
    }
}

#[derive(Debug, Clone)]
enum AppEvent {
    ReceiveErr,
    IgnoredEvent,
    QuitKeyEvent,
    ProcessEnded(String, String, Pid, Pid, Option<ExitStatus>),
}

fn wait_for_term(out_chan: &Sender<AppEvent>, running_p: &RunningProgram) -> JoinHandle<()> {
    let rp = (*running_p).clone();
    let tx = out_chan.clone();
    thread::spawn(move || {
        let s: sysinfo::System = sysinfo::System::new_all();
        let p_proc = s.process(rp.program.program_pid);
        if let Some(_p_pid) = p_proc {
            let stat = p_proc.unwrap().wait();
            let _ = tx.send(AppEvent::ProcessEnded(
                rp.spec.name,
                rp.program.session_name,
                rp.program.tmux_pid,
                rp.program.program_pid,
                stat,
            ));
        } else {
            let _ = tx.send(AppEvent::ProcessEnded(
                rp.spec.name,
                rp.program.session_name,
                rp.program.tmux_pid,
                rp.program.program_pid,
                None,
            ));
        }
    })
}

fn start_event_loop(out_chan: &Sender<AppEvent>, die_chan: Receiver<()>) -> JoinHandle<()> {
    let tx = out_chan.clone();
    thread::spawn(move || {
        loop {
            let ep = event::poll(Duration::from_millis(200));
            match ep {
                Ok(true) => {
                    if let Ok(ev) = event::read() {
                        match ev {
                            Event::Key(ke) => {
                                if ke.code == KeyCode::Char('q') {
                                    let _ = tx.send(AppEvent::QuitKeyEvent);
                                } else {
                                    let _ = tx.send(AppEvent::IgnoredEvent);
                                }
                            }
                            _ => {
                                let _ = tx.send(AppEvent::IgnoredEvent);
                            }
                        }
                    } else {
                        let _ = tx.send(AppEvent::ReceiveErr);
                    }
                }
                Ok(false) => {
                    if let Ok(_e) = die_chan.try_recv() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(AppEvent::ReceiveErr);
                }
            }
        }
    })
}

fn check_for_message(ds: &DisplayStatus) -> Option<AppEvent> {
    if ds.outstanding_pids.is_empty() {
        return None;
    }
    if let Ok(msg) = ds.child_event_listener.recv() {
        Some(msg)
    } else {
        Some(AppEvent::ReceiveErr)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();

    let exe_loc = std::env::current_dir().unwrap();
    let exe_path = exe_loc.canonicalize().unwrap();

    let config = try_load_config(&exe_path, &mut args)?;

    let mut started_commands: Vec<StartedProgram> = Vec::new();
    let tab_adapter = choose_tab_adapter()?;
    let mut display_status = DisplayStatus::new(tab_adapter);
    for spec in config.apps.iter() {
        let comm = start_command(&config.namespace, &spec)?;
        started_commands.push(comm);
        display_status.mark_app_started(&spec.name);
    }
    let running_programs = convert_pids(&started_commands)?;
    display_status.start_running(&running_programs);
    let mut terminal = ratatui::init();
    while let Some(evt) = check_for_message(&display_status) {
        match evt {
            AppEvent::ProcessEnded(s, s_name, _t_pid, p_pid, _) => {
                display_status.mark_app_dead(&s, &s_name, &p_pid);
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
            AppEvent::QuitKeyEvent => {
                display_status.execute_quit();
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
            _ => {
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
        }
    }
    display_status.finish_shutdown();
    ratatui::restore();
    Ok(())
}
