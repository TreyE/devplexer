use std::{
    process::ExitStatus,
    sync::mpsc::Sender,
    thread::{self, JoinHandle},
};

use sysinfo::Pid;

use crate::tmux::RunningProgram;

pub(crate) enum AppStatus {
    Started,
    Running(Pid),
    Dead(Pid),
}

#[derive(Debug, Clone)]
pub(crate) enum AppEvent {
    ReceiveErr,
    IgnoredEvent,
    QuitKeyEvent,
    LogEvent(Vec<u8>),
    #[allow(dead_code)]
    ProcessEnded(String, String, Pid, Pid, Option<ExitStatus>),
}

pub(crate) fn wait_for_term(
    out_chan: &Sender<AppEvent>,
    running_p: &RunningProgram,
) -> JoinHandle<()> {
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
