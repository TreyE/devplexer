use std::{
    error::Error,
    process::ExitStatus,
    sync::mpsc::{Receiver, Sender},
};

mod iterm;

mod config;

use sysinfo::Pid;

mod tabadapter;

mod tmux;

use std::sync::mpsc::channel;
use std::thread;

use crate::{
    config::try_load_config,
    iterm::ITermTabAdapter,
    tabadapter::TabAdapter,
    tmux::{RunningProgram, StartedProgram, cleanup_session, convert_pids, start_command},
};

#[derive(Debug, Clone)]
enum TmuxProcessOutcome {
    ReceiveErr,
    ProcessEnded(String, Pid, Pid, Option<ExitStatus>),
}

fn choose_tab_adapter() -> Result<Option<impl TabAdapter>, Box<dyn Error>> {
    let ta = ITermTabAdapter::new()?;
    Ok(Some(ta))
}

fn wait_for_term(out_chan: &Sender<TmuxProcessOutcome>, running_p: &RunningProgram) {
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
    });
}

fn check_for_message(
    rx: &Receiver<TmuxProcessOutcome>,
    outstanding_pids: &Vec<Pid>,
) -> Option<TmuxProcessOutcome> {
    if outstanding_pids.is_empty() {
        return None;
    }
    if let Ok(msg) = rx.recv() {
        Some(msg)
    } else {
        Some(TmuxProcessOutcome::ReceiveErr)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args();
    if args.len() < 2 {
        println!("{:?}", args);
        return Ok(());
    }

    let exe_loc = std::env::current_dir().unwrap();
    let f_name = args.nth_back(0).unwrap();
    let exe_path = exe_loc.canonicalize().unwrap();

    let config = try_load_config(&exe_path, &f_name)?;

    let mut started_commands: Vec<StartedProgram> = Vec::new();
    for spec in config.apps.iter() {
        let comm = start_command(&config.namespace, &spec)?;
        println!("App Starting: {}", spec.name);
        started_commands.push(comm);
    }
    let mut running_programs = convert_pids(&started_commands)?;
    let mut tab_adapter = choose_tab_adapter()?;
    if let Some(ta) = tab_adapter.as_mut() {
        for c in running_programs.iter_mut() {
            ta.open(&c.program.session_name);
        }
    }
    let (tx, rx) = channel::<TmuxProcessOutcome>();
    let mut outstanding_pids = Vec::new();
    let mut dead_sessions = Vec::new();
    for c in running_programs.iter() {
        outstanding_pids.push(c.program.program_pid);
        wait_for_term(&tx, &c);
    }
    println!("{:?}", outstanding_pids);
    while let Some(evt) = check_for_message(&rx, &outstanding_pids) {
        match evt {
            TmuxProcessOutcome::ProcessEnded(s, _t_pid, p_pid, _) => {
                println!("Process Died: {s} - PID {p_pid}");
                outstanding_pids.retain(|f| f != &p_pid);
                dead_sessions.push(s);
            }
            _ => {}
        }
    }
    for ds in dead_sessions.iter() {
        cleanup_session(ds);
        if let Some(ta) = tab_adapter.as_mut() {
            ta.close(ds);
        }
    }
    Ok(())
}
