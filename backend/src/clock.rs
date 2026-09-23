//! The server's wall clock, with an offset tests can advance.

use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};

#[derive(Debug)]
pub struct Clock {
    offset: Mutex<Duration>,
}

impl Clock {
    pub fn system() -> Self {
        Clock {
            offset: Mutex::new(Duration::zero()),
        }
    }

    pub fn now(&self) -> DateTime<Utc> {
        Utc::now() + *self.offset.lock().expect("clock lock")
    }

    pub fn advance(&self, by: Duration) {
        *self.offset.lock().expect("clock lock") += by;
    }
}
