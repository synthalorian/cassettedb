//! Crash reporter module for CassetteDB.
//!
//! Captures panics and backtraces to aid debugging and stability testing.

use backtrace::Backtrace;
use std::sync::Once;

static CRASH_REPORTER_INIT: Once = Once::new();

/// Initialize the global panic hook that captures backtraces.
pub fn install_panic_hook() {
    CRASH_REPORTER_INIT.call_once(|| {
        std::panic::set_hook(Box::new(panic_hook));
    });
}

fn panic_hook(info: &std::panic::PanicHookInfo) {
    let bt = Backtrace::new();
    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!("CassetteDB has encountered a fatal error and crashed.");
    eprintln!();
    if let Some(location) = info.location() {
        eprintln!("  Location: {}:{}", location.file(), location.line());
    }
    if let Some(message) = info.payload().downcast_ref::<&str>() {
        eprintln!("  Message:  {}", message);
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        eprintln!("  Message:  {}", message);
    }
    eprintln!();
    eprintln!("Backtrace:");
    eprintln!("{:?}", bt);
    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

/// Generate a crash report string for programmatic use.
pub fn capture_crash_report(info: &std::panic::PanicHookInfo) -> String {
    let bt = Backtrace::new();
    let mut report = String::new();
    report.push_str("CassetteDB Crash Report\n");
    report.push_str("========================\n\n");
    if let Some(location) = info.location() {
        report.push_str(&format!("Location: {}:{}\n", location.file(), location.line()));
    }
    if let Some(message) = info.payload().downcast_ref::<&str>() {
        report.push_str(&format!("Message:  {}\n", message));
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        report.push_str(&format!("Message:  {}\n", message));
    }
    report.push_str("\nBacktrace:\n");
    report.push_str(&format!("{:?}", bt));
    report
}
