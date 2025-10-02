pub(crate) trait TabAdapter {
    fn open(&mut self, session_name: &str);
    fn close(&mut self, session_name: &str);
}
