use std::error::Error;

use crate::tabadapter::iterm::ITermTabAdapter;

pub(crate) mod iterm;

pub(crate) trait TabAdapter {
    fn open(&mut self, session_name: &str);
    fn after_all_open(&mut self);
    fn close(&mut self, session_name: &str);
    fn after_all_closed(&mut self);
}

pub(crate) fn choose_tab_adapter() -> Result<Option<Box<dyn TabAdapter>>, Box<dyn Error>> {
    let ta = ITermTabAdapter::new()?;
    Ok(Some(Box::new(ta)))
}
