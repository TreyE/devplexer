use std::error::Error;

#[cfg(target_os = "macos")]
mod iterm;

#[cfg(target_os = "macos")]
mod osx_terminal;

#[cfg(target_os = "macos")]
use crate::tabadapter::iterm::ITermTabAdapter;

#[cfg(target_os = "macos")]
use crate::tabadapter::iterm::iterm_installed;

#[cfg(target_os = "macos")]
use crate::tabadapter::osx_terminal::OsxTerminalAdapter;

use log::info;

pub(crate) trait TabAdapter {
    fn open(&mut self, session_name: &str);
    fn after_all_open(&mut self);
    fn close(&mut self, session_name: &str);
    fn after_all_closed(&mut self);
}

#[cfg(target_os = "macos")]
pub(crate) fn choose_tab_adapter() -> Result<Option<Box<dyn TabAdapter>>, Box<dyn Error>> {
    if iterm_installed() {
        let ta = ITermTabAdapter::new()?;
        info!("Booted ITerm adapter.");
        return Ok(Some(Box::new(ta)));
    }

    let ta = OsxTerminalAdapter::new()?;
    info!("Booted Terminal Adapter");
    Ok(Some(Box::new(ta)))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn choose_tab_adapter() -> Result<Option<Box<dyn TabAdapter>>, Box<dyn Error>> {
    info!("No adapter available.");
    Ok(None)
}
