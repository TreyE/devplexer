use std::{collections::HashMap, error::Error, ffi::OsStr, os::unix::ffi::OsStringExt};

use osakit::{Script, Value};
use tmux_interface::AttachSession;

use crate::tabadapter::TabAdapter;

pub(crate) struct ITermTabAdapter {
    current_session: Value,
    iterm_mappings: HashMap<String, Value>,
}

impl ITermTabAdapter {
    pub(crate) fn new() -> Result<Self, Box<dyn Error>> {
        let cs = get_original_session()?;
        Ok(ITermTabAdapter {
            current_session: cs,
            iterm_mappings: HashMap::new(),
        })
    }
}

impl TabAdapter for ITermTabAdapter {
    fn open(&mut self, session_name: &str) {
        let spawn_res = spawn_iterm_tab(session_name);
        if let Ok(sr) = spawn_res {
            self.iterm_mappings.insert(session_name.to_owned(), sr);
        }
        let _ = refocus_original_session(&self.current_session);
    }

    fn close(&mut self, session_name: &str) {
        if let Some(v) = self.iterm_mappings.get(session_name) {
            let _ = cleanup_iterm_tab(v);
            self.iterm_mappings.remove(session_name);
        }
    }
}

fn get_original_session() -> Result<Value, Box<dyn Error>> {
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on get_original_tab()
            tell application \"iTerm\"
    	       activate
               if not(exists window 1)
                 return null
               end if
    	       tell current window
                   set t to current tab
             	   set sess to (current session of t)
                   set sid to (id of sess)
               end tell
            end tell
            return sid
         end get_original_tab",
    );
    script.compile()?;
    let r = script.execute_function("get_original_tab", vec![]);
    println!("{:?}", r);
    if r.is_err() {
        return Ok(Value::Null);
    }
    Ok(r.unwrap())
}

fn refocus_original_session(t: &Value) -> Result<(), Box<dyn Error>> {
    if t.is_null() {
        return Ok(());
    }
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on focus_original_tab(x)
            tell application \"iTerm\"
               	activate
               	repeat with aWindow in windows
         			tell aWindow
            				repeat with aTab in tabs
               					tell aTab
              						repeat with aSession in sessions
             							if id of aSession is x then
                                          select aWindow
                                          select aTab
                						  select aSession
             							  return
             							end if
              						end repeat
               					end tell
            				end repeat
         			end tell
               	end repeat
            end tell
        end focus_original_tab",
    );
    script.compile()?;
    let _r = script.execute_function("focus_original_tab", vec![t.clone()])?;
    Ok(())
}

fn cleanup_iterm_tab(t: &Value) -> Result<(), Box<dyn Error>> {
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on close_tmux_tab(x)
            tell application \"iTerm\"
               	activate
               	repeat with aWindow in windows
         			tell aWindow
            				repeat with aTab in tabs
               					tell aTab
              						repeat with aSession in sessions
             							if id of aSession is x then
                								tell aSession
             									  close
             									  return
                								end tell
             							end if
              						end repeat
               					end tell
            				end repeat
         			end tell
               	end repeat
            end tell
        end close_tmux_tab",
    );
    script.compile()?;
    let _r = script.execute_function("close_tmux_tab", vec![t.clone()])?;
    Ok(())
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
    let cmd_string: String = String::from_utf8(encoded_string)?;
    let cmd_str = osakit::Value::String(cmd_string);
    let mut script = Script::new_from_source(
        osakit::Language::AppleScript,
        "on look_at_tmux(x)
            tell application \"iTerm\"
    	       activate
               set cTab to null
               if not (exists window 1) then
                 create window with default profile
                 tell current window
                   set cTab to current tab
                 end tell
               end if
    	       tell current window
                 set t to (create tab with default profile)
    			 set sess to (current session of t)
    		     set sid to (id of sess)
			     tell sess
				   write text x
			     end tell
    	       end tell
               if cTab is not null
                 tell cTab
                   close
                 end
               end
            end tell
            return sid
         end look_at_tmux",
    );
    script.compile()?;
    let r = script.execute_function("look_at_tmux", vec![cmd_str])?;
    Ok(r)
}
