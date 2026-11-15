//! The three screens: session list, "new session" prompt, multi-select kill.
//! Every frame is built into one String and flushed with a single write().

use crate::tmux::{sanitize, Session};
use crate::tty::{Key, Tty};
use std::time::{SystemTime, UNIX_EPOCH};

const SEL_BG: &str = "\x1b[48;5;237m";
const OFF: &str = "\x1b[22;39m"; // reset intensity + fg, keep bg
const RESET: &str = "\x1b[0m";

pub enum Outcome {
    Open(String),
    Create(String),
    Kill(Vec<String>),
    Quit,
}

// ───────────────────────────── drawing helpers ─────────────────────────────

struct Line {
    s: String,
    w: usize,
}

impl Line {
    fn new() -> Self {
        Line { s: String::with_capacity(160), w: 0 }
    }
    fn raw(&mut self, esc: &str) {
        self.s.push_str(esc);
    }
    fn put(&mut self, sgr: &str, text: &str) {
        if sgr.is_empty() {
            self.s.push_str(text);
        } else {
            self.s.push_str("\x1b[");
            self.s.push_str(sgr);
            self.s.push('m');
            self.s.push_str(text);
            self.s.push_str(OFF);
        }
        self.w += text.chars().count();
    }
    fn spaces(&mut self, n: usize) {
        for _ in 0..n {
            self.s.push(' ');
        }
        self.w += n;
    }
}

fn fit(s: &str, w: usize) -> String {
    let clean = |c: char| if c.is_control() { ' ' } else { c };
    if s.chars().count() <= w {
        s.chars().map(clean).collect()
    } else {
        let mut o: String = s.chars().take(w.saturating_sub(1)).map(clean).collect();
        o.push('…');
        o
    }
}

fn render(tty: &Tty, lines: &[Line], cursor: Option<(usize, usize)>) {
    let (rows, _) = tty.size();
    let mut out = String::with_capacity(2048);
    out.push_str("\x1b[?25l\x1b[H");
    for (i, l) in lines.iter().take(rows).enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        out.push_str(&l.s);
        out.push_str(RESET);
        out.push_str("\x1b[K");
    }
    out.push_str("\x1b[J");
    if let Some((r, c)) = cursor {
        out.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1));
    }
    tty.write(&out);
}

fn age(now: i64, t: i64) -> String {
    let d = (now - t).max(0);
    if d < 60 {
        "now".into()
    } else if d < 3600 {
        format!("{}m", d / 60)
    } else if d < 86400 {
        format!("{}h", d / 3600)
    } else {
        format!("{}d", d / 86400)
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn header(title: &str, sgr: &str, extra: &str) -> Line {
    let mut l = Line::new();
    l.put("", " ");
    l.put(sgr, title);
    if !extra.is_empty() {
        l.put("2", &format!("  {extra}"));
    }
    l
}

fn footer(text: &str, sgr: &str, cols: usize) -> Line {
    let mut l = Line::new();
    l.put("", " ");
    l.put(sgr, &fit(text, cols.saturating_sub(2)));
    l
}

// ───────────────────────────── menu (list / kill) ──────────────────────────

struct Menu<'a> {
    ss: &'a [Session],
    cur: Option<&'a str>,
    /// Some => kill mode with checkboxes.
    marks: Option<&'a [bool]>,
    /// Append a "+ new session" row.
    with_new: bool,
}

#[allow(clippy::too_many_arguments)]
fn draw_menu(
    tty: &Tty,
    m: &Menu,
    title: &str,
    title_sgr: &str,
    sel: usize,
    top: &mut usize,
    foot: &str,
    foot_sgr: &str,
) {
    let (rows, cols) = tty.size();
    let total = m.ss.len() + m.with_new as usize;
    let h = rows.saturating_sub(4).max(1);
    if sel < *top {
        *top = sel;
    } else if sel >= *top + h {
        *top = sel + 1 - h;
    }

    let compact = cols < 40;
    let box_w = if m.marks.is_some() { 4 } else { 0 };
    // marker(3) + idx(2) + box + gap(2) + win(6) + gap(2) + badge(8) + gap(2) + age(4)
    let fixed = 3 + 2 + box_w + if compact { 0 } else { 2 + 6 + 2 + 8 + 2 + 4 };
    let avail = cols.saturating_sub(fixed + 1).max(4);
    let namew = m.ss.iter().map(|s| s.name.chars().count()).max().unwrap_or(0).clamp(4, avail);
    let t = now();

    let count = format!("{} session{}", m.ss.len(), if m.ss.len() == 1 { "" } else { "s" });
    let mut lines = vec![header(title, title_sgr, &count), Line::new()];

    for i in *top..(*top + h).min(total) {
        let selected = i == sel;
        let mut l = Line::new();
        if selected {
            l.raw(SEL_BG);
        }
        l.put(if selected { "1;36" } else { "" }, if selected { " ❯ " } else { "   " });

        if i < m.ss.len() {
            let s = &m.ss[i];
            if let Some(marks) = m.marks {
                if marks[i] {
                    l.put("1;31", "[x] ");
                } else {
                    l.put("2", "[ ] ");
                }
            }
            if i < 9 {
                l.put("2", &format!("{} ", i + 1));
            } else {
                l.spaces(2);
            }
            let nm = fit(&s.name, namew);
            let nm_w = nm.chars().count();
            l.put(if selected { "1" } else { "" }, &nm);
            if !compact {
                l.spaces(namew - nm_w + 2);
                l.put("2", &format!("{:>2} win", s.windows));
                l.spaces(2);
                if Some(s.id.as_str()) == m.cur {
                    l.put("1;32", "current ");
                } else if s.attached > 0 {
                    l.put("33", "attached");
                } else {
                    l.spaces(8);
                }
                l.spaces(2);
                l.put("2", &age(t, s.activity));
            }
        } else {
            l.put(if selected { "1;32" } else { "32" }, "+ new session");
        }
        if selected {
            let w = l.w;
            l.spaces(cols.saturating_sub(1).saturating_sub(w));
        }
        lines.push(l);
    }

    lines.push(Line::new());
    lines.push(footer(foot, foot_sgr, cols));
    render(tty, &lines, None);
}

/// Shared navigation keys. Returns Some(new_sel) if the key moved the cursor.
fn nav(key: Key, sel: usize, total: usize, page: usize) -> Option<usize> {
    Some(match key {
        Key::Up | Key::Char('k') | Key::Ctrl(b'p') => {
            if sel == 0 {
                total - 1
            } else {
                sel - 1
            }
        }
        Key::Down | Key::Char('j') | Key::Ctrl(b'n') => (sel + 1) % total,
        Key::Home | Key::Char('g') => 0,
        Key::End | Key::Char('G') => total - 1,
        Key::PageUp => sel.saturating_sub(page),
        Key::PageDown => (sel + page).min(total - 1),
        _ => return None,
    })
}

fn page_size(tty: &Tty) -> usize {
    tty.size().0.saturating_sub(4).max(1)
}

// ───────────────────────────── list screen ─────────────────────────────────

/// Most recently active session that isn't the one we're in (alt-tab behaviour).
fn initial_sel(ss: &[Session], cur: Option<&str>) -> usize {
    ss.iter()
        .enumerate()
        .filter(|(_, s)| Some(s.id.as_str()) != cur)
        .max_by_key(|(_, s)| s.activity)
        .map(|(i, _)| i)
        .unwrap_or(if cur.is_some() { ss.len() } else { 0 })
}

pub fn list(tty: &Tty, ss: &[Session], cur: Option<&str>, default_name: &str) -> Outcome {
    if ss.is_empty() {
        return match new_prompt(tty, ss, default_name) {
            Some(n) => Outcome::Create(n),
            None => Outcome::Quit,
        };
    }
    let menu = Menu { ss, cur, marks: None, with_new: true };
    let total = ss.len() + 1;
    let mut sel = initial_sel(ss, cur);
    let mut top = 0;
    const HINT: &str = "↑↓/jk move · ⏎ open · 1-9 jump · n new · q quit";

    loop {
        draw_menu(tty, &menu, "tmux sessions", "1", sel, &mut top, HINT, "2");
        let key = tty.key();
        if let Some(s) = nav(key, sel, total, page_size(tty)) {
            sel = s;
            continue;
        }
        match key {
            Key::Tab => sel = (sel + 1) % total,
            Key::Enter if sel < ss.len() => return Outcome::Open(ss[sel].name.clone()),
            Key::Enter | Key::Char('n') | Key::Char('+') => {
                if let Some(n) = new_prompt(tty, ss, default_name) {
                    return Outcome::Create(n);
                }
            }
            Key::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < ss.len() {
                    return Outcome::Open(ss[i].name.clone());
                }
            }
            Key::Esc | Key::Char('q') | Key::Ctrl(b'c') | Key::Eof => return Outcome::Quit,
            _ => {}
        }
    }
}

// ───────────────────────────── new-session prompt ──────────────────────────

pub fn new_prompt(tty: &Tty, ss: &[Session], default_name: &str) -> Option<String> {
    let mut text = String::new();
    const PROMPT: &str = "name ❯ ";
    loop {
        let (_, cols) = tty.size();
        let effective = if text.trim().is_empty() { default_name } else { text.trim() };
        let exists = ss.iter().any(|s| s.name == effective);

        let mut lines = vec![header("new session", "1;32", ""), Line::new()];
        let mut l = Line::new();
        l.put("", "   ");
        l.put("36", PROMPT);
        if text.is_empty() {
            l.put("2", &fit(default_name, cols.saturating_sub(12)));
        } else {
            l.put("", &fit(&text, cols.saturating_sub(12)));
        }
        lines.push(l);

        let mut hint = Line::new();
        if exists {
            hint.put("", "   ");
            hint.put("33", &fit(&format!("“{effective}” already exists — ⏎ switches to it"), cols.saturating_sub(5)));
        }
        lines.push(hint);
        lines.push(Line::new());
        lines.push(footer("⏎ create · esc cancel · empty = folder name", "2", cols));

        let cursor_col = 3 + PROMPT.chars().count() + text.chars().count().min(cols.saturating_sub(12));
        render(tty, &lines, Some((2, cursor_col)));

        match tty.key() {
            Key::Enter => {
                let n = sanitize(effective);
                if !n.is_empty() {
                    return Some(n);
                }
            }
            Key::Esc | Key::Ctrl(b'c') | Key::Eof => return None,
            Key::Backspace => {
                text.pop();
            }
            Key::Ctrl(b'u') => text.clear(),
            Key::Ctrl(b'w') => {
                let t = text.trim_end().len();
                text.truncate(text[..t].rfind(' ').map_or(0, |i| i + 1));
            }
            Key::Char(c) if text.chars().count() < 64 => {
                text.push(if c == '.' || c == ':' { '_' } else { c });
            }
            _ => {}
        }
    }
}

// ───────────────────────────── kill screen ─────────────────────────────────

pub fn kill_menu(tty: &Tty, ss: &[Session], cur: Option<&str>) -> Option<Vec<String>> {
    let total = ss.len();
    let mut marks = vec![false; total];
    let mut sel = 0;
    let mut top = 0;
    let mut confirm = false;
    const HINT: &str = "space mark · tab mark+next · a all · ⏎ kill · esc cancel";

    loop {
        let n = marks.iter().filter(|&&m| m).count();
        let has_cur = ss.iter().zip(&marks).any(|(s, &m)| m && Some(s.id.as_str()) == cur);
        let menu = Menu { ss, cur, marks: Some(&marks), with_new: false };
        if confirm {
            let msg = format!(
                "Kill {n} session{}{}? press y to confirm, any other key cancels",
                if n == 1 { "" } else { "s" },
                if has_cur { " (includes the current one)" } else { "" }
            );
            draw_menu(tty, &menu, "kill sessions", "1;31", sel, &mut top, &msg, "1;31");
        } else {
            draw_menu(tty, &menu, "kill sessions", "1;31", sel, &mut top, HINT, "2");
        }

        let key = tty.key();
        if confirm {
            match key {
                Key::Resize => {}
                Key::Char('y') | Key::Char('Y') => {
                    return Some(
                        ss.iter().zip(&marks).filter(|(_, &m)| m).map(|(s, _)| s.name.clone()).collect(),
                    );
                }
                _ => confirm = false,
            }
            continue;
        }

        if let Some(s) = nav(key, sel, total, page_size(tty)) {
            sel = s;
            continue;
        }
        match key {
            Key::Char(' ') => marks[sel] = !marks[sel],
            Key::Tab => {
                marks[sel] = !marks[sel];
                sel = (sel + 1) % total;
            }
            Key::Char('a') => {
                let all = marks.iter().all(|&m| m);
                marks.fill(!all);
            }
            Key::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < total {
                    marks[i] = !marks[i];
                }
            }
            Key::Enter => {
                if n == 0 {
                    marks[sel] = true; // nothing marked: act on the highlighted row
                }
                confirm = true;
            }
            Key::Esc | Key::Char('q') | Key::Ctrl(b'c') | Key::Eof => return None,
            _ => {}
        }
    }
}
