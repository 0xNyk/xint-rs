use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const FRAMES: &[&str] = &[
    "\u{2807}", "\u{280b}", "\u{2819}", "\u{2838}", "\u{2834}", "\u{2826}", "\u{2847}", "\u{280f}",
];

pub struct Spinner {
    running: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    is_tty: bool,
}

impl Spinner {
    pub fn new(message: &str) -> Self {
        let is_tty = std::io::stderr().is_terminal();
        if !is_tty {
            let _ = writeln!(std::io::stderr(), "{message}");
            return Self {
                running: Arc::new(AtomicBool::new(false)),
                handle: None,
                is_tty: false,
            };
        }

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let msg = message.to_string();

        let handle = thread::spawn(move || {
            let mut frame = 0usize;
            let mut stderr = std::io::stderr();
            while running_clone.load(Ordering::Relaxed) {
                let _ = write!(stderr, "\r\x1b[K{} {msg}", FRAMES[frame % FRAMES.len()]);
                let _ = stderr.flush();
                frame += 1;
                thread::sleep(Duration::from_millis(80));
            }
        });

        Self {
            running,
            handle: Some(handle),
            is_tty,
        }
    }

    pub fn done(self, message: &str) {
        self.stop();
        if self.is_tty {
            let _ = write!(
                std::io::stderr(),
                "\r\x1b[K\x1b[32m\u{2713}\x1b[0m {message}\n"
            );
            let _ = std::io::stderr().flush();
        } else {
            let _ = writeln!(std::io::stderr(), "{message}");
        }
    }

    pub fn fail(self, message: &str) {
        self.stop();
        if self.is_tty {
            let _ = write!(
                std::io::stderr(),
                "\r\x1b[K\x1b[31m\u{2717}\x1b[0m {message}\n"
            );
            let _ = std::io::stderr().flush();
        } else {
            let _ = writeln!(std::io::stderr(), "{message}");
        }
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if self.is_tty {
            let _ = write!(std::io::stderr(), "\r\x1b[K");
            let _ = std::io::stderr().flush();
        }
    }
}
