//! Presses the Ledger's buttons in Speculos the way an owner approving
//! every request would, through Speculos's REST API on 127.0.0.1:5000.
//!
//! On a Nano, each request is a row of screens: the owner presses right to
//! read them and both buttons on the one that approves. The approving
//! screens are the ones Ledger's own tests for the Bitcoin app wait for
//! (`tests/instructions.py` at 2.4.6), as they read on a Nano X: "Approve
//! public key" (any screen starting "Approve"), "Register account",
//! "Sign transaction", and "Continue anyway" past a warning. A screen
//! starting "Reject" comes after the approving one, so the approver steps
//! back from it rather than wait there.
//!
//! Each new screen is written to standard error as it appears, and a Ledger
//! call that hasn't finished after [`PATIENCE`] stops the test run with the
//! screens so far: a request the approver can't finish would otherwise wait
//! forever. Every long string (keys, addresses, codes) is cut out first, so
//! nothing written can hold a key or the wallet's approval code.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const API: &str = "127.0.0.1:5000";
const APPROVE: [&str; 3] = ["Register account", "Sign transaction", "Continue anyway"];
const POLL: Duration = Duration::from_millis(250);
/// How long one Ledger call may take, approvals included.
const PATIENCE: Duration = Duration::from_secs(180);

/// Approves whatever the Ledger asks while it runs.
pub struct Approver {
    stop: Arc<AtomicBool>,
    seen: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl Approver {
    /// Starts watching. The screen showing now is the app's idle screen,
    /// which is left alone.
    pub fn start() -> Self {
        let idle = screen().expect("Speculos's API on 127.0.0.1:5000");
        trace(&format!("idle screen: {}", redacted(&idle)));
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (flag, log) = (stop.clone(), seen.clone());
        let handle = thread::spawn(move || {
            while !flag.load(Ordering::SeqCst) {
                thread::sleep(POLL);
                let Some(lines) = screen() else { continue };
                if lines == idle {
                    continue;
                }
                let shown = redacted(&lines);
                let mut log = log.lock().unwrap();
                if log.last() != Some(&shown) {
                    trace(&format!("screen: {shown}"));
                    log.push(shown);
                }
                drop(log);
                press(button_for(&lines));
            }
        });
        Self { stop, seen, handle: Some(handle) }
    }

    /// Stops watching and returns the screens seen, redacted.
    pub fn finish(mut self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Approver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Runs `call` while approving on the Ledger. If it fails, prints the
/// screens the Ledger showed (redacted) before returning the error. If it
/// takes longer than [`PATIENCE`], stops the whole test run.
pub fn approving<T, E: std::fmt::Display>(call: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    let approver = Approver::start();
    let done = Arc::new(AtomicBool::new(false));
    let watchdog = {
        let (done, seen) = (done.clone(), approver.seen.clone());
        thread::spawn(move || {
            let started = Instant::now();
            while !done.load(Ordering::SeqCst) {
                if started.elapsed() > PATIENCE {
                    trace(&format!("The Ledger call didn't finish in {} s. Its screens, long strings removed:", PATIENCE.as_secs()));
                    for s in seen.lock().unwrap().iter() {
                        trace(&format!("  | {s}"));
                    }
                    std::process::exit(101);
                }
                thread::sleep(POLL);
            }
        })
    };
    let result = call();
    done.store(true, Ordering::SeqCst);
    let _ = watchdog.join();
    let screens = approver.finish();
    if let Err(e) = &result {
        eprintln!("The Ledger call failed ({e}). Its screens, long strings removed:");
        for s in &screens {
            eprintln!("  | {s}");
        }
    }
    result
}

/// Straight to standard error, past the test harness's capture, so it shows
/// even for a test that never finishes. Only ever redacted text.
fn trace(line: &str) {
    let _ = writeln!(std::io::stderr(), "[speculos] {line}");
}

/// Which button an approving owner presses on this screen.
fn button_for(lines: &[String]) -> &'static str {
    let first = lines.first().map(|l| l.trim()).unwrap_or_default();
    if first.starts_with("Approve") || lines.iter().any(|l| APPROVE.contains(&l.trim())) {
        "both"
    } else if first.starts_with("Reject") {
        "left"
    } else {
        "right"
    }
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

/// The screen's lines with anything that could be part of a key, address or
/// code replaced by "…": everything below the title of a key or address
/// screen, and elsewhere a run of letters and digits 16 or more long, or 6
/// or more long with a digit or a capital after its first letter. A Nano
/// splits a long key over several lines, so its pieces are short; words and
/// amounts are kept.
fn redacted(lines: &[String]) -> String {
    // On a screen showing a key or an address, everything below the title
    // goes, whatever it looks like: the last piece of a key can be short.
    let title = lines.first().map(|l| l.to_ascii_lowercase()).unwrap_or_default();
    let keyish = title.contains("key") || title.starts_with("to") || title.contains("address");
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if keyish && i > 0 {
            out.push_str(" / …");
            continue;
        }
        let mut run = String::new();
        let mut cleaned = String::new();
        for c in line.chars().chain(std::iter::once(' ')) {
            if c.is_ascii_alphanumeric() {
                run.push(c);
                continue;
            }
            cleaned.push_str(if looks_like_key_material(&run) { "…" } else { &run });
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

fn looks_like_key_material(run: &str) -> bool {
    let mixed = run.chars().any(|c| c.is_ascii_digit()) || run.chars().skip(1).any(|c| c.is_ascii_uppercase());
    run.len() >= 16 || (run.len() >= 6 && mixed)
}

#[cfg(test)]
#[test]
fn redaction_removes_keys_and_codes() {
    let line = |s: &str| vec![s.to_owned()];
    assert_eq!(redacted(&line("Key 1")), "Key 1");
    assert_eq!(redacted(&line("tpubDChvZNVMps14g7P2zZQU6xQHGCMn8tKqiEyDXq")), "…");
    assert_eq!(redacted(&line("[e54c53b1/87'/1'/0']tpubDChvZNVMps14g7P2zZQU6x")), "[…/87'/1'/0']…");
    assert_eq!(redacted(&line(&"ab".repeat(32))), "…");
    // A key's pieces on a Nano's lines, and the wallet's words, side by side.
    assert_eq!(redacted(&line("ZQU6xQHGCMn8 tKqiEy")), "… …");
    assert_eq!(redacted(&line("Register account")), "Register account");
    assert_eq!(redacted(&line("Fees 0.00000402 BTC")), "Fees 0.… BTC");
    assert_eq!(redacted(&[String::from("Sign transaction"), String::from("Satkeep Savings")]), "Sign transaction / Satkeep Savings");
    // A key's or an address's last, short piece, as a Nano X showed one.
    assert_eq!(redacted(&[String::from("Dummy key@0 (3/3)"), String::from("55N9D")]), "Dummy key@0 (3/3) / …");
    assert_eq!(redacted(&[String::from("To (2/2)"), String::from("q9x")]), "To (2/2) / …");
}

#[cfg(test)]
#[test]
fn approves_only_on_the_approving_screens() {
    let screen = |lines: &[&str]| lines.iter().map(|l| l.to_string()).collect::<Vec<_>>();
    // As the Bitcoin Test app 2.4.6 shows a public key on a Nano X.
    assert_eq!(button_for(&screen(&["Confirm public key"])), "right");
    assert_eq!(button_for(&screen(&["Path", "87'/1'/0'"])), "right");
    assert_eq!(button_for(&screen(&["Public key (1/3)", "tpubD"])), "right");
    assert_eq!(button_for(&screen(&["Approve public key"])), "both");
    assert_eq!(button_for(&screen(&["Reject operation"])), "left");
    assert_eq!(button_for(&screen(&["Register account"])), "both");
    assert_eq!(button_for(&screen(&["Sign transaction"])), "both");
    assert_eq!(button_for(&screen(&["Amount", "0.00001"])), "right");
}
