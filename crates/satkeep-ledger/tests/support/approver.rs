//! Presses the Ledger's buttons in Speculos the way an owner approving
//! every request would, through Speculos's REST API on 127.0.0.1:5000.
//!
//! On a Nano, each request is a row of screens: the owner presses right to
//! read them and both buttons on the one that approves. The approving
//! screens' labels are the ones Ledger's own tests for the Bitcoin app wait
//! for (`tests/instructions.py` at 2.4.6): "Approve" for a public key,
//! "Register account" for a wallet, "Sign transaction" for a payment, and
//! "Continue anyway" past a warning.
//!
//! What the screens said is kept for a failing test to show, with every long
//! string (keys, addresses, codes) cut out first, so nothing printed can hold
//! a key or the wallet's approval code.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const API: &str = "127.0.0.1:5000";
const APPROVE: [&str; 4] = ["Approve", "Register account", "Sign transaction", "Continue anyway"];
const POLL: Duration = Duration::from_millis(250);

/// Approves whatever the Ledger asks while it runs.
pub struct Approver {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<Vec<String>>>,
}

impl Approver {
    /// Starts watching. The screen showing now is the app's idle screen,
    /// which is left alone.
    pub fn start() -> Self {
        let idle = screen().expect("Speculos's API on 127.0.0.1:5000");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let handle = thread::spawn(move || {
            let mut seen: Vec<String> = Vec::new();
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(POLL);
                let Some(lines) = screen() else { continue };
                if lines == idle {
                    continue;
                }
                let shown = redacted(&lines);
                if seen.last() != Some(&shown) {
                    seen.push(shown);
                }
                let approves = lines.iter().any(|l| APPROVE.contains(&l.trim()));
                press(if approves { "both" } else { "right" });
            }
            seen
        });
        Self { stop, handle: Some(handle) }
    }

    /// Stops watching and returns the screens seen, redacted.
    pub fn finish(mut self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        self.handle.take().map(|h| h.join().unwrap_or_default()).unwrap_or_default()
    }
}

impl Drop for Approver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Runs `call` while approving on the Ledger. If it fails, prints the
/// screens the Ledger showed (redacted) before returning the error.
pub fn approving<T, E: std::fmt::Display>(call: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let approver = Approver::start();
    let result = call();
    let screens = approver.finish();
    if let Err(e) = &result {
        eprintln!("The Ledger call failed ({e}). Its screens, long strings removed:");
        for s in &screens {
            eprintln!("  | {s}");
        }
    }
    result
}

/// The text lines on the screen now.
fn screen() -> Option<Vec<String>> {
    let body = request("GET /events?currentscreenonly=true", "")?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let events = json.get("events")?.as_array()?;
    Some(events.iter().filter_map(|e| e.get("text")?.as_str().map(str::to_owned)).collect())
}

fn press(button: &str) {
    let _ = request(&format!("POST /button/{button}"), r#"{"action":"press-and-release"}"#);
    thread::sleep(POLL);
}

/// One HTTP/1.0 request, so the answer is never chunked. Returns the body.
fn request(line: &str, body: &str) -> Option<String> {
    let mut stream = TcpStream::connect(API).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let head = format!(
        "{line} HTTP/1.0\r\nHost: {API}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(body.as_bytes()).ok()?;
    let mut answer = String::new();
    stream.read_to_string(&mut answer).ok()?;
    answer.split_once("\r\n\r\n").map(|(_, body)| body.to_owned())
}

/// The screen's lines with every run of 16 or more letters and digits
/// replaced by "…": keys, addresses and codes are all that long.
fn redacted(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines {
        let mut run = String::new();
        let mut cleaned = String::new();
        for c in line.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphanumeric() {
                run.push(c);
                continue;
            }
            cleaned.push_str(if run.len() >= 16 { "…" } else { &run });
            run.clear();
            cleaned.push(c);
        }
        if !out.is_empty() {
            out.push_str(" / ");
        }
        out.push_str(cleaned.trim_end());
    }
    out
}

#[cfg(test)]
#[test]
fn redaction_removes_keys_and_codes() {
    let line = |s: &str| vec![s.to_owned()];
    assert_eq!(redacted(&line("Key 1")), "Key 1");
    assert_eq!(redacted(&line("tpubDChvZNVMps14g7P2zZQU6xQHGCMn8tKqiEyDXq")), "…");
    assert_eq!(redacted(&line("[e54c53b1/87'/1'/0']tpubDChvZNVMps14g7P2zZQU6x")), "[e54c53b1/87'/1'/0']…");
    assert_eq!(redacted(&line(&"ab".repeat(32))), "…");
}
