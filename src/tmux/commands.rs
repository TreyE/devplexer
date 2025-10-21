use std::{error::Error, ffi::OsStr, os::unix::ffi::OsStringExt};

use tmux_interface::{AttachSession, KillSession};

pub(crate) fn cleanup_session(session_name: &str) {
    let _ = KillSession::new()
        .target_session(session_name)
        .build()
        .into_tmux()
        .status();
}

pub(crate) fn attach_session_command_for_cli(session_name: &str) -> Result<String, Box<dyn Error>> {
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
    Ok(String::from_utf8(encoded_string)?)
}
