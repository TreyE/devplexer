pub(crate) mod iterm;

pub(crate) trait TabAdapter {
    fn open(&mut self, session_name: &str);
    fn after_all_open(&mut self);
    fn close(&mut self, session_name: &str);
    fn after_all_closed(&mut self);
}
