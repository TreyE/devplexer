use std::time::{Duration, SystemTime};

use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

use crate::tmux::send_interrupt;

pub(crate) fn kill_with_timeout(
    system: &mut System,
    pid: &Pid,
    sigs: &[Signal],
    time_to_wait: Duration,
) {
    let mut timeup = false;
    let mut start_at;
    let process = system.process(pid.clone());
    if let None = process {
        return;
    }
    for s in sigs.iter() {
        start_at = SystemTime::now();
        timeup = false;
        let fp = system.process(pid.clone());
        if let Some(process) = fp {
            let _ = process.kill_with(s.clone());
        } else {
            return;
        }
        let _ = system.refresh_processes(ProcessesToUpdate::Some(&[pid.clone()]), true);
        while let Some(_proc) = system.process(pid.clone())
            && !timeup
        {
            std::thread::sleep(Duration::from_millis(100));
            timeup = start_at.elapsed().unwrap_or(Duration::from_millis(0)) >= time_to_wait;
            let _ = system.refresh_processes(ProcessesToUpdate::Some(&[pid.clone()]), true);
        }
        if !timeup {
            return;
        }
    }
    if timeup {
        if let Some(process) = system.process(pid.clone()) {
            let _ = process.kill_with_and_wait(Signal::Kill);
        }
    }
}

pub(crate) fn kill_process(pid: &Pid, session_name: &Option<String>) {
    let mut s: sysinfo::System = sysinfo::System::new_all();
    let p_proc = s.process(pid.clone());

    if let Some(_process) = p_proc {
        if let Some(sn) = session_name {
            send_interrupt(&sn);
            let mut timedout = false;
            let start_at = SystemTime::now();
            while let Some(_p) = s.process(pid.clone())
                && !timedout
            {
                std::thread::sleep(Duration::from_millis(100));
                let _ = s.refresh_processes(ProcessesToUpdate::Some(&[pid.clone()]), true);
                timedout = start_at.elapsed().unwrap_or(Duration::from_millis(0))
                    >= Duration::from_millis(2000);
            }
        }

        if let Some(_proc) = s.process(pid.clone()) {
            kill_with_timeout(
                &mut s,
                pid,
                &[Signal::Interrupt, Signal::Term],
                Duration::from_millis(3000),
            );
        }
    }
}
