use std::{
    borrow::Cow,
    collections::HashMap,
    error::Error,
    ffi::{OsStr, OsString},
    io::{BufRead, Cursor},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    process::{Child, ExitStatus},
    str::FromStr,
};

use osakit::{Script, Value};
use tmux_interface::{
    AttachSession, ListSessions, NewSession, NewWindow, StdIO, Tmux, TmuxCommands,
};

struct ProgramSpec {
    working_directory: PathBuf,
    command: String,
    name: String,
}

struct StartedTmuxProgram {
    session_name: String,
    status: ExitStatus,
}

struct RunningTmuxProgram {
    session_name: String,
    pid: sysinfo::Pid,
}

struct StartedProgram {
    working_directory: PathBuf,
    command: String,
    program: StartedTmuxProgram,
}

struct RunningProgram {
    working_directory: PathBuf,
    command: String,
    program: RunningTmuxProgram,
}

fn convert_pids(
    started_commands: &Vec<StartedProgram>,
) -> Result<Vec<RunningProgram>, Box<dyn Error>> {
    let mut running_programs: Vec<RunningProgram> = Vec::new();
    let mut cs = ListSessions::new()
        .format("#{session_name}: #{pid}")
        .build()
        .into_tmux()
        .into_command();
    let output = cs.output()?;
    let entries = output.stdout.lines();
    let mut pid_mapping: HashMap<String, sysinfo::Pid> = HashMap::new();
    for entry in entries {
        if let Some((name, pid)) = entry?.split_once(": ") {
            let pid_c = u32::from_str(pid)?;
            let upid = sysinfo::Pid::from_u32(pid_c);
            pid_mapping.insert(name.to_owned(), upid);
        }
    }
    for sc in started_commands.iter() {
        let sn = sc.program.session_name.clone();
        let pm = pid_mapping.get(&sn).unwrap();
        let rp = RunningProgram {
            working_directory: sc.working_directory.clone(),
            command: sc.command.clone(),
            program: RunningTmuxProgram {
                session_name: sn,
                pid: pm.clone(),
            },
        };
        running_programs.push(rp);
    }
    Ok(running_programs)
}

fn spawn_iterm_tab(session_name: &str) -> Result<Value, Box<dyn Error>> {
    let cmd = AttachSession::new()
        .target_session(session_name)
        .detach_other()
        .build()
        .into_tmux()
        .into_command();
    let cmd_args: Vec<&OsStr> = cmd.get_args().collect();
    let mut encoded_string = cmd.get_program().to_os_string().into_encoded_bytes();
    encoded_string.extend(" ".as_bytes());
    encoded_string.extend(cmd_args.join(OsStr::new(" ")).into_vec());
    //encoded_string.extend("; exit".as_bytes());
    let cmd_string: String = String::from_utf8(encoded_string)?;
    println!("{:?}", cmd_string);
    let cmd_str = osakit::Value::String(cmd_string);
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on look_at_tmux(x)
            tell application \"iTerm\"
    	       activate
    	       tell current window
    		     set t to (create tab with default profile)
    			 set sess to (current session of t)
    		     set wid to id
    		     set sid to (id of sess)
			     tell sess
				   write text x
			     end tell
    	       end tell
            end tell
            return {windowid:wid, sessionid:sid}
         end look_at_tmux",
    );
    script.compile()?;
    let r = script.execute_function("look_at_tmux", vec![cmd_str])?;
    println!("{:?}", r);
    Ok(r)
}

fn build_spec_with_base(
    name: &str,
    base_dir: &str,
    working_directory: &str,
    command: &str,
) -> Result<ProgramSpec, Box<dyn Error>> {
    let working_dir;
    let relative_part = std::path::PathBuf::from_str(working_directory)?;
    if relative_part.is_absolute() {
        working_dir = relative_part;
    } else {
        working_dir = std::path::PathBuf::from_str(base_dir)?.join(relative_part);
    }
    Ok(ProgramSpec {
        name: name.to_owned(),
        command: command.to_owned(),
        working_directory: working_dir.to_owned(),
    })
}

fn start_command(
    session_name: &str,
    p_spec: &ProgramSpec,
) -> Result<StartedProgram, Box<dyn Error>> {
    let s_name = session_name.to_owned() + "-" + &p_spec.name;
    let s_cmd = NewSession::new()
        .detached()
        .session_name(&s_name)
        .start_directory(p_spec.working_directory.as_os_str().to_string_lossy())
        .shell_command(&p_spec.command);
    let tmux = s_cmd.build().into_tmux();
    let estatus = tmux.status()?;
    Ok(StartedProgram {
        working_directory: p_spec.working_directory.clone(),
        command: p_spec.command.clone(),
        program: StartedTmuxProgram {
            session_name: s_name,
            status: estatus,
        },
    })
}

static PROGSPECS: [(&str, &str, &str); 2] = [
    (
        "ui-tailwind",
        "/Users/tevans/proj/localstack-viewer/ui",
        "source ~/.bashrc; nvm use system; npx @tailwindcss/cli -i ./input.css -o ./assets/tailwind.css --watch",
    ),
    ("ui", "/Users/tevans/proj/localstack-viewer/ui", "dx serve"),
];

fn main() -> Result<(), Box<dyn Error>> {
    let mut started_commands: Vec<StartedProgram> = Vec::new();
    for (n, d, c) in PROGSPECS.iter() {
        let spec = build_spec_with_base(n, "", d, c)?;
        let comm = start_command("localstack-viewer", &spec)?;
        started_commands.push(comm);
    }
    let mut running_programs = convert_pids(&started_commands)?;
    let mut s = sysinfo::System::new_all();
    for c in running_programs.iter_mut() {
        let _c = spawn_iterm_tab(&c.program.session_name);
    }
    for c in running_programs.iter_mut() {
        let proc = s.process(c.program.pid);
        let rs = proc.unwrap().wait();
        println!("{:?}", rs);
    }
    Ok(())
}
