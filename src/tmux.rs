//! Thin wrapper around the `tmux` CLI.

use std::env;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

pub struct Session {
    pub id: String,
    pub name: String,
    pub windows: u32,
    pub attached: u32,
    pub activity: i64,
}

/// Are we running inside a tmux client (popup, pane, ...)?
pub fn inside() -> bool {
    env::var_os("TMUX").is_some_and(|v| !v.is_empty())
}

/// tmux silently rewrites '.' and ':' in session names; do it ourselves so
/// existence checks and targets match.
pub fn sanitize(name: &str) -> String {
    name.trim().replace(['.', ':'], "_")
}

const FMT: &str = "#{session_id} #{session_windows} #{session_attached} #{session_activity} #{session_name}";

/// Returns (sessions, current_session_id). One tmux process, one round trip.
/// No server running => empty list (not an error).
pub fn list() -> io::Result<(Vec<Session>, Option<String>)> {
    let inside = inside();
    let mut cmd = Command::new("tmux");
    if inside {
        // Chained in a single invocation: first line = current session id.
        cmd.args(["display-message", "-p", "#{session_id}", ";"]);
    }
    cmd.args(["list-sessions", "-F", FMT]);
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    let out = cmd.output()?;
    if !out.status.success() {
        return Ok((Vec::new(), None));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let current = if inside { lines.next().map(|s| s.trim().to_string()) } else { None };

    let mut sessions = Vec::new();
    for line in lines {
        let mut it = line.splitn(5, ' ');
        let (Some(id), Some(w), Some(a), Some(act), Some(name)) =
            (it.next(), it.next(), it.next(), it.next(), it.next())
        else {
            continue;
        };
        sessions.push(Session {
            id: id.to_string(),
            name: name.to_string(),
            windows: w.parse().unwrap_or(0),
            attached: a.parse().unwrap_or(0),
            activity: act.parse().unwrap_or(0),
        });
    }
    Ok((sessions, current))
}

/// Switch to / attach an existing session. Replaces this process; only
/// returns on failure.
pub fn open(name: &str) -> io::Error {
    let target = format!("={name}");
    let mut cmd = Command::new("tmux");
    if inside() {
        cmd.args(["switch-client", "-t", &target]);
    } else {
        cmd.args(["attach-session", "-t", &target]);
    }
    cmd.exec()
}

/// Create a session (if needed) and go to it. Replaces this process; only
/// returns on failure.
pub fn create(name: &str, exists: bool) -> io::Error {
    if exists {
        return open(name);
    }
    let mut cmd = Command::new("tmux");
    if inside() {
        let target = format!("={name}");
        cmd.args(["new-session", "-d", "-s", name, ";", "switch-client", "-t", &target]);
    } else {
        cmd.args(["new-session", "-s", name]);
    }
    cmd.exec()
}

/// Kill sessions by exact name. Returns (name, ok) per session.
/// `current_name` is killed last so we don't cut off our own branch early.
pub fn kill(names: &[String], current_name: Option<&str>) -> Vec<(String, bool)> {
    let mut order: Vec<&String> = names.iter().filter(|n| Some(n.as_str()) != current_name).collect();
    order.extend(names.iter().filter(|n| Some(n.as_str()) == current_name));
    order
        .into_iter()
        .map(|n| {
            let ok = Command::new("tmux")
                .args(["kill-session", "-t", &format!("={n}")])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            (n.clone(), ok)
        })
        .collect()
}
