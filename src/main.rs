use std::{
    collections::HashMap,
    error::Error,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
    time::Duration,
};

mod config;

mod apps;

use log::{error, info};

mod logging;

use sysinfo::Pid;

mod tabadapter;

mod tmux;

mod processes;

use ratatui::{
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Flex, Layout},
    style::Stylize,
    text::Text,
    widgets::{Paragraph, Row, Table, Widget},
};
use std::sync::mpsc::channel;
use std::thread;

use crate::{
    apps::{AppEvent, AppStatus, wait_for_term},
    config::try_load_config,
    logging::{LogBuffer, initialize_logger},
    processes::kill_process,
    tabadapter::{TabAdapter, choose_tab_adapter},
    tmux::{RunningProgram, StartedProgram, cleanup_session, convert_pids, start_command},
};

struct DisplayStatus<'a> {
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
    child_event_sender: &'a Sender<AppEvent>,
    logbuffer: LogBuffer,
}

impl<'a> DisplayStatus<'a> {
    fn new(
        ta: Option<Box<dyn TabAdapter>>,
        ces: &'a Sender<AppEvent>,
        cel: Receiver<AppEvent>,
    ) -> Self {
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
            logbuffer: LogBuffer::new(),
        }
    }

    fn mark_app_started(&mut self, app_name: &str) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Started);
    }

    fn mark_app_running(&mut self, app_name: &str, session_name: &str, pid: &Pid) {
        self.outstanding_pids.push(pid.clone());
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Running(pid.clone()));
        self.pid_map.insert(pid.clone(), session_name.to_owned());
    }

    fn mark_app_dead(&mut self, app_name: &str, session_name: &str, pid: &Pid) {
        self.app_statuses
            .insert(app_name.to_owned(), AppStatus::Dead(pid.clone()));
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
            info!("Shutting down adapter.");
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
            info!("Shutting down tmux sessions and processes.");
            let mut kps = Vec::new();
            for p in self.outstanding_pids.iter() {
                let the_process = p.clone();
                let session_name = self.pid_map.get(&the_process);
                let owned_sn = session_name.map(|s| s.to_owned());
                info!(
                    "Shutting down session named: {} - PID {}",
                    session_name.unwrap_or(&"N/A".to_owned()),
                    p
                );
                kps.push(thread::spawn(move || {
                    kill_process(&the_process, &owned_sn);
                }));
            }
            self.killer_procs = Some(kps);
        }
    }

    fn add_log_entry(&mut self, data: &Vec<u8>) {
        self.logbuffer.write_data(data);
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

impl<'a> Widget for &DisplayStatus<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let mut rows = Vec::new();
        let n_cell = Text::raw("Name").left_aligned();
        let p_cell = Text::raw("PID").centered();
        let s_cell = Text::raw("Status");
        let title_row = Row::from_iter(vec![n_cell, p_cell, s_cell])
            .underlined()
            .bold();
        rows.push(title_row);
        for (aname, astatus) in self.app_statuses.iter() {
            let row_vals = match astatus {
                AppStatus::Dead(rp) => vec![
                    Text::raw(aname.to_owned()),
                    Text::raw(rp.to_string()).right_aligned(),
                    Text::raw("❌".to_owned()).right_aligned(),
                ],
                AppStatus::Running(rp) => vec![
                    Text::raw(aname.to_owned()),
                    Text::raw(rp.to_string()).right_aligned(),
                    Text::raw("🚀".to_owned()).right_aligned(),
                ],
                _ => vec![
                    Text::raw(aname.to_owned()),
                    Text::raw("N/A".to_owned()).right_aligned(),
                    Text::raw("🛫".to_owned()).right_aligned(),
                ],
            };
            let row = Row::from_iter(row_vals);
            rows.push(row);
        }
        let widths = vec![
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(6),
        ];
        let table = Table::new(rows, widths);
        let tlayout = Layout::vertical(vec![Constraint::Length(
            (self.app_statuses.len() + 1) as u16,
        )])
        .flex(Flex::Center);
        let vlayouttop = Layout::vertical(vec![
            Constraint::Fill(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .split(area);
        let hlayout = Layout::horizontal(vec![Constraint::Fill(1)]).flex(Flex::Center);
        let [help_area] = hlayout.areas(vlayouttop[2]);
        let [log_area] = hlayout.areas(vlayouttop[1]);
        let [t_area] = hlayout.areas(tlayout.split(vlayouttop[0])[0]);
        let p = Paragraph::new("Q - Quit").centered();
        let log_string = Vec::from_iter(self.logbuffer.data_queue.iter().map(|f| f.clone()));
        let str = unsafe { String::from_utf8_unchecked(log_string) };
        let log_p = Paragraph::new(str);
        log_p.render(log_area, buf);
        table.render(t_area, buf);
        p.render(help_area, buf);
    }
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

fn create_app_event_channel() -> (&'static Sender<AppEvent>, Receiver<AppEvent>) {
    let (s, r) = channel::<AppEvent>();
    (Box::leak(Box::new(s)), r)
}

fn main() -> Result<(), Box<dyn Error>> {
    let (aes, aer) = create_app_event_channel();
    initialize_logger(aes);
    let mut args = std::env::args();

    let exe_loc = std::env::current_dir().unwrap();
    let exe_path = exe_loc.canonicalize().unwrap();

    let config = try_load_config(&exe_path, &mut args)?;
    info!("Loaded configuration.");
    let mut started_commands: Vec<StartedProgram> = Vec::new();
    let tab_adapter = choose_tab_adapter()?;
    let mut display_status = DisplayStatus::new(tab_adapter, &aes, aer);

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
                error!("Application Died: {}", s);
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
            AppEvent::QuitKeyEvent => {
                info!("Shutdown Request Received.");
                display_status.execute_quit();
                terminal.draw(|f| f.render_widget(&display_status, f.area()))?;
            }
            AppEvent::LogEvent(ld) => {
                display_status.add_log_entry(&ld);
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
