use std::{collections::HashMap, error::Error};

use osakit::{Script, Value};

use crate::{tabadapter::TabAdapter, tmux::attach_session_command_for_cli};

pub(crate) struct OsxTerminalAdapter {
    terminal_mappings: HashMap<String, Value>,
}

impl OsxTerminalAdapter {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        Ok(OsxTerminalAdapter {
            terminal_mappings: HashMap::new(),
        })
    }
}

impl TabAdapter for OsxTerminalAdapter {
    fn open(&mut self, session_name: &str) {
        let spawn_res = spawn_terminal_tab(session_name);
        if let Ok(sr) = spawn_res {
            self.terminal_mappings.insert(session_name.to_owned(), sr);
        }
    }

    fn close(&mut self, session_name: &str) {
        if let Some(v) = self.terminal_mappings.get(session_name) {
            let _ = cleanup_terminal_tab(v);
            self.terminal_mappings.remove(session_name);
        }
    }

    fn after_all_open(&mut self) {
        //let _ = refocus_original_session(&self.current_session);
    }

    fn after_all_closed(&mut self) {
        //let _ = refocus_original_session(&self.current_session);
    }
}

fn spawn_terminal_tab(session_name: &str) -> Result<Value, Box<dyn Error>> {
    let cmd_string = attach_session_command_for_cli(session_name)?;
    let cmd_str = osakit::Value::String(cmd_string);
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on look_at_tmux(x)
            tell application \"Terminal\"
    	       activate
               set currentTab1 to (do script x)
               repeat with theWindow in windows
                 if frontmost of theWindow then
                   set currentWindowId to id of theWindow
                   return currentWindowId
                   exit repeat
                 end if
               end repeat
            end tell
         end look_at_tmux",
    );
    script.compile()?;
    let r = script.execute_function("look_at_tmux", vec![cmd_str])?;
    Ok(r)
}

fn cleanup_terminal_tab(t: &Value) -> Result<(), Box<dyn Error>> {
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on close_tmux_tab(x)
            tell application \"Terminal\"
               	activate
               	repeat with aWindow in windows
                      if (id of aWindow) is x
                        tell aWindow
                          close
                          return
                        end tell
                      end if
               	end repeat
            end tell
        end close_tmux_tab",
    );
    script.compile()?;
    let _r = script.execute_function("close_tmux_tab", vec![t.clone()]);
    Ok(())
}
