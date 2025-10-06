use std::error::Error;

#[cfg(target_os = "macos")]
mod iterm;

#[cfg(target_os = "macos")]
use crate::tabadapter::iterm::ITermTabAdapter;

pub(crate) trait TabAdapter {
    fn open(&mut self, session_name: &str);
    fn after_all_open(&mut self);
    fn close(&mut self, session_name: &str);
    fn after_all_closed(&mut self);
}

#[cfg(target_os = "macos")]
pub(crate) fn choose_tab_adapter() -> Result<Option<Box<dyn TabAdapter>>, Box<dyn Error>> {
    let ta = ITermTabAdapter::new()?;
    Ok(Some(Box::new(ta)))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn choose_tab_adapter() -> Result<Option<Box<dyn TabAdapter>>, Box<dyn Error>> {
    Ok(None)
}
