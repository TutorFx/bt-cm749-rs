//! Where progress messages of the install/uninstall flows go: the terminal by default,
//! or the session log while the interactive wizard shows spinners instead.

use std::fmt;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

static LOG: Mutex<Option<File>> = Mutex::new(None);

/// Sends subsequent messages to `log` (`None` goes back to the terminal).
pub fn redirect(log: Option<File>) {
    *LOG.lock().unwrap_or_else(|e| e.into_inner()) = log;
}

pub fn line(args: fmt::Arguments<'_>) {
    match LOG.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        Some(file) => {
            let _ = writeln!(file, "{args}");
        }
        None => println!("{args}"),
    }
}

/// `println!` for progress messages; see the module docs.
#[macro_export]
macro_rules! say {
    ($($arg:tt)*) => { $crate::ui::line(format_args!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_to_the_log_and_back() {
        let log = tempfile::NamedTempFile::new().unwrap();
        redirect(Some(log.reopen().unwrap()));
        say!("step {}", 1);
        redirect(None);
        assert_eq!(std::fs::read_to_string(log.path()).unwrap(), "step 1\n");
    }
}
