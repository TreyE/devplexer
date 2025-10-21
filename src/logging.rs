use std::{
    collections::VecDeque,
    io::Write,
    sync::{Arc, Mutex, mpsc::Sender},
};

use log::Log;
use simplelog::{Config, WriteLogger};

use crate::AppEvent;

struct WritableClearableLog {
    inner: Arc<Mutex<Vec<u8>>>,
}

pub(crate) struct EventLogger<'a> {
    event_sender: &'a Sender<AppEvent>,
    writer: Arc<Mutex<Vec<u8>>>,
    write_logger: Box<WriteLogger<WritableClearableLog>>,
}

impl Write for WritableClearableLog {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.inner.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> EventLogger<'a> {
    pub(crate) fn new(sender: &'a Sender<AppEvent>) -> Self {
        let r_vec = Arc::new(Mutex::new(Vec::new()));
        let wcl = WritableClearableLog {
            inner: r_vec.clone(),
        };
        EventLogger {
            writer: r_vec,
            event_sender: sender,
            write_logger: WriteLogger::new(log::LevelFilter::Trace, Config::default(), wcl),
        }
    }
}

impl<'a> Log for EventLogger<'a> {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        {
            let l = self.writer.lock();
            l.unwrap().clear();
        };
        self.write_logger.log(record);
        let ls = self.writer.lock().unwrap().clone();
        let _ = self.event_sender.send(AppEvent::LogEvent(ls));
    }

    fn flush(&self) {}
}

pub(crate) struct LogBuffer {
    pub(crate) data_queue: VecDeque<u8>,
}

impl LogBuffer {
    pub(crate) fn new() -> Self {
        LogBuffer {
            data_queue: VecDeque::with_capacity(512),
        }
    }

    pub(crate) fn write_data(&mut self, data: &Vec<u8>) {
        if data.len() > 512 {
            self.data_queue.clear();
            let start_n = data.len() - 512;
            self.data_queue.write_all(&data[start_n..]).unwrap();
        } else if self.data_queue.len() + data.len() > 512 {
            let dropped_length = (self.data_queue.len() + data.len()) - 512;
            self.data_queue.drain(0..dropped_length);
            self.data_queue.write_all(data.as_slice()).unwrap();
        } else {
            self.data_queue.write_all(data.as_slice()).unwrap();
        }
    }
}

fn create_event_logger(aes: &'static Sender<AppEvent>) -> &'static dyn Log {
    let el = EventLogger::new(&aes);
    Box::leak(Box::new(el))
}

pub(crate) fn initialize_logger(aes: &'static Sender<AppEvent>) {
    let logger = create_event_logger(aes);
    log::set_logger(&*logger).unwrap();
    log::set_max_level(log::LevelFilter::Info);
}
