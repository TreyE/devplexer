use std::{collections::HashMap, error::Error, io::BufRead, str::FromStr};

use tmux_interface::{KillSession, ListSessions, NewSession, SendKeys, Tmux};

use crate::config::ProgramSpec;

#[derive(Debug)]
pub(crate) enum ProgramStartErrors {
    ProgramDiedEarlyError(String),
}

impl std::fmt::Display for ProgramStartErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&format!("{:?}", self))
    }
}

impl std::error::Error for ProgramStartErrors {}

#[derive(Clone)]
pub(crate) struct RunningTmuxProgram {
    pub(crate) command: String,
    pub(crate) session_name: String,
    pub(crate) tmux_pid: sysinfo::Pid,
    pub(crate) program_pid: sysinfo::Pid,
}

#[derive(Clone)]
pub(crate) struct StartedProgram {
    pub(crate) spec: ProgramSpec,
    pub(crate) command: String,
    pub(crate) session_name: String,
}

#[derive(Clone)]
pub(crate) struct RunningProgram {
    pub(crate) spec: ProgramSpec,
    pub(crate) program: RunningTmuxProgram,
}

pub(crate) fn cleanup_session(session_name: &str) {
    let _ = KillSession::new()
        .target_session(session_name)
        .build()
        .into_tmux()
        .status();
}

pub(crate) fn convert_pids(
    started_commands: &Vec<StartedProgram>,
) -> Result<Vec<RunningProgram>, Box<dyn Error>> {
    let mut running_programs: Vec<RunningProgram> = Vec::new();
    let mut cs = ListSessions::new()
        .format("#{session_name}: #{pid}: #{pane_pid}")
        .build()
        .into_tmux()
        .into_command();
    let output = cs.output()?;
    let entries = output.stdout.lines();
    let mut pid_mapping: HashMap<String, (sysinfo::Pid, sysinfo::Pid)> = HashMap::new();
    for entry in entries {
        if let Some((name, pids)) = entry?.split_once(": ") {
            if let Some((tmux_pid, pane_pid)) = pids.split_once(": ") {
                let pid_t = u32::from_str(tmux_pid)?;
                let pid_c = u32::from_str(pane_pid)?;
                let upid = sysinfo::Pid::from_u32(pid_t);
                let cpid = sysinfo::Pid::from_u32(pid_c);
                pid_mapping.insert(name.to_owned(), (upid, cpid));
            }
        }
    }
    for sc in started_commands.iter() {
        let sn = sc.session_name.clone();
        let pm = pid_mapping
            .get(&sn)
            .ok_or_else(|| ProgramStartErrors::ProgramDiedEarlyError(sn.clone()))?;
        let rp = RunningProgram {
            spec: sc.spec.clone(),
            program: RunningTmuxProgram {
                command: sc.command.clone(),
                session_name: sn,
                tmux_pid: pm.0,
                program_pid: pm.1,
            },
        };
        running_programs.push(rp);
    }
    Ok(running_programs)
}

pub(crate) fn send_interrupt(session_name: &str) {
    let _ = SendKeys::new()
        .target_pane(session_name)
        .key("C-c")
        .build()
        .into_tmux()
        .status();
}

pub(crate) fn start_command(
    session_name: &str,
    p_spec: &ProgramSpec,
) -> Result<StartedProgram, Box<dyn Error>> {
    let s_name = session_name.to_owned() + "-" + &p_spec.name;

    let command_with_remain =
        format!("tmux set-option -t {} remain-on-exit on; ", s_name) + &p_spec.command;

    let s_cmd = NewSession::new()
        .detached()
        .session_name(&s_name)
        .start_directory(p_spec.working_directory.as_os_str().to_string_lossy())
        .shell_command(command_with_remain.clone());
    let tmux = s_cmd.build().into_tmux(); //.into_tmux();
    let _estatus = tmux.status()?;
    Ok(StartedProgram {
        spec: p_spec.clone(),
        command: command_with_remain,
        session_name: s_name,
    })
}
