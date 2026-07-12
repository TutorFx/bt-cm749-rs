//! SIGINT/SIGTERM handling, the Rust counterpart of the scripts' `trap ... INT TERM`.
//!
//! Signals only set a flag: the process keeps running so it can wait for the current
//! child, forward the signal to it and then roll back before exiting.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

static RECEIVED: OnceLock<Arc<AtomicUsize>> = OnceLock::new();
static SUPPRESSED: AtomicBool = AtomicBool::new(false);

/// Installs the handlers. Idempotent.
pub fn install() -> std::io::Result<()> {
    if RECEIVED.get().is_some() {
        return Ok(());
    }
    let flag = Arc::new(AtomicUsize::new(0));
    for sig in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register_usize(sig, Arc::clone(&flag), sig as usize)?;
    }
    let _ = RECEIVED.set(flag);
    Ok(())
}

/// The pending signal number, unless suppressed (during rollback).
pub fn pending() -> Option<i32> {
    if SUPPRESSED.load(Ordering::SeqCst) {
        return None;
    }
    RECEIVED.get().map(|f| f.load(Ordering::SeqCst)).filter(|&s| s != 0).map(|s| s as i32)
}

/// Stops reporting and forwarding signals so a rollback can run to completion.
pub fn suppress() {
    SUPPRESSED.store(true, Ordering::SeqCst);
}
