//! External command execution behind a trait so orchestration logic can be unit-tested
//! with a recording fake instead of touching dkms, package managers or modprobe.

use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process};

use crate::error::{Error, Result};
use crate::signals;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
}

impl Cmd {
    pub fn new(program: impl Into<String>) -> Self {
        Cmd { program: program.into(), args: Vec::new() }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }
}

impl fmt::Display for Cmd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for a in &self.args {
            write!(f, " {a}")?;
        }
        Ok(())
    }
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
}

pub trait Runner {
    /// Runs with inherited stdio and returns the exit code.
    fn status(&self, cmd: &Cmd) -> Result<i32>;
    /// Runs capturing stdout (stderr discarded).
    fn output(&self, cmd: &Cmd) -> Result<Output>;
    /// Resolves `program` on PATH.
    fn which(&self, program: &str) -> Option<PathBuf>;

    fn exists(&self, program: &str) -> bool {
        self.which(program).is_some()
    }

    /// Runs and fails with the command's own exit code when it is non-zero.
    fn run(&self, cmd: &Cmd) -> Result<()> {
        match self.status(cmd)? {
            0 => Ok(()),
            code => Err(Error::CommandFailed { cmd: cmd.to_string(), code }),
        }
    }

    /// Best-effort run: failures (including a missing binary) are ignored.
    fn run_quiet(&self, cmd: &Cmd) {
        let _ = self.output(cmd);
    }
}

pub struct SystemRunner;

impl SystemRunner {
    fn wait(cmd: &Cmd, mut command: Command) -> Result<std::process::Output> {
        let spawn_err = |source| Error::Spawn { cmd: cmd.to_string(), source };
        let mut child = command.spawn().map_err(spawn_err)?;
        let mut forwarded = false;
        // Poll so a SIGTERM aimed only at us still reaches the child (SIGINT from a
        // terminal already hits the whole foreground process group).
        loop {
            if child.try_wait().map_err(spawn_err)?.is_some() {
                break;
            }
            if !forwarded && signals::pending().is_some() {
                if let Some(pid) = Pid::from_raw(child.id() as i32) {
                    let _ = kill_process(pid, Signal::TERM);
                }
                forwarded = true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let output = child.wait_with_output().map_err(spawn_err)?;
        if let Some(sig) = signals::pending() {
            return Err(Error::Interrupted(sig));
        }
        Ok(output)
    }

    fn code(status: std::process::ExitStatus) -> i32 {
        use std::os::unix::process::ExitStatusExt;
        status.code().or_else(|| status.signal().map(|s| 128 + s)).unwrap_or(1)
    }
}

impl Runner for SystemRunner {
    fn status(&self, cmd: &Cmd) -> Result<i32> {
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args);
        Ok(Self::code(Self::wait(cmd, command)?.status))
    }

    fn output(&self, cmd: &Cmd) -> Result<Output> {
        let mut command = Command::new(&cmd.program);
        command.args(&cmd.args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        let out = Self::wait(cmd, command)?;
        Ok(Output { code: Self::code(out.status), stdout: String::from_utf8_lossy(&out.stdout).into_owned() })
    }

    fn which(&self, program: &str) -> Option<PathBuf> {
        which::which(program).ok()
    }
}

#[cfg(test)]
pub mod testing {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;

    /// Records every command line and answers with scripted results
    /// (exit 0 / empty stdout by default; every program "exists").
    #[derive(Default)]
    pub struct RecordingRunner {
        pub calls: RefCell<Vec<String>>,
        responses: HashMap<String, (i32, String)>,
        missing: Vec<String>,
    }

    impl RecordingRunner {
        /// Scripts the answer for any command line starting with `prefix`.
        pub fn respond(mut self, prefix: &str, code: i32, stdout: &str) -> Self {
            self.responses.insert(prefix.into(), (code, stdout.into()));
            self
        }

        pub fn missing(mut self, program: &str) -> Self {
            self.missing.push(program.into());
            self
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }

        fn answer(&self, cmd: &Cmd) -> Result<(i32, String)> {
            let line = cmd.to_string();
            self.calls.borrow_mut().push(line.clone());
            if self.missing.contains(&cmd.program) {
                return Err(Error::Spawn { cmd: line, source: std::io::ErrorKind::NotFound.into() });
            }
            let best = self.responses.iter().filter(|(p, _)| line.starts_with(p.as_str())).max_by_key(|(p, _)| p.len());
            Ok(best.map(|(_, r)| r.clone()).unwrap_or_default())
        }
    }

    impl Runner for RecordingRunner {
        fn status(&self, cmd: &Cmd) -> Result<i32> {
            Ok(self.answer(cmd)?.0)
        }

        fn output(&self, cmd: &Cmd) -> Result<Output> {
            let (code, stdout) = self.answer(cmd)?;
            Ok(Output { code, stdout })
        }

        fn which(&self, program: &str) -> Option<PathBuf> {
            (!self.missing.iter().any(|m| m == program)).then(|| PathBuf::from("/usr/bin").join(program))
        }
    }
}
