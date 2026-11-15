//! Minimal raw-terminal layer on top of libc. Talks to /dev/tty directly so it
//! works inside `tmux display-popup` and with redirected stdio.

use std::io;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};

static WINCH: AtomicBool = AtomicBool::new(false);

extern "C" fn on_winch(_sig: libc::c_int) {
    WINCH.store(true, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Esc,
    Backspace,
    Tab,
    Char(char),
    /// Ctrl + lowercase letter, e.g. `Ctrl(b'c')`.
    Ctrl(u8),
    Resize,
    Eof,
    Other,
}

pub struct Tty {
    fd: libc::c_int,
    orig: libc::termios,
}

impl Tty {
    /// Open /dev/tty, enter raw mode + alternate screen, hide the cursor.
    pub fn open() -> io::Result<Tty> {
        unsafe {
            let fd = libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            let mut orig = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(fd, orig.as_mut_ptr()) != 0 {
                let e = io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }
            let orig = orig.assume_init();
            let mut raw = orig;
            libc::cfmakeraw(&mut raw);
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            if libc::tcsetattr(fd, libc::TCSANOW, &raw) != 0 {
                let e = io::Error::last_os_error();
                libc::close(fd);
                return Err(e);
            }

            // SIGWINCH without SA_RESTART so a blocking read() returns EINTR.
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = on_winch as *const () as usize;
            libc::sigemptyset(&mut sa.sa_mask);
            sa.sa_flags = 0;
            libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());

            let tty = Tty { fd, orig };
            tty.write("\x1b[?1049h\x1b[?25l");
            Ok(tty)
        }
    }

    pub fn write(&self, s: &str) {
        let mut b = s.as_bytes();
        while !b.is_empty() {
            let n = unsafe { libc::write(self.fd, b.as_ptr().cast(), b.len()) };
            if n < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                break;
            }
            b = &b[n as usize..];
        }
    }

    /// (rows, cols)
    pub fn size(&self) -> (usize, usize) {
        unsafe {
            let mut ws: libc::winsize = std::mem::zeroed();
            if libc::ioctl(self.fd, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 && ws.ws_col > 0
            {
                (ws.ws_row as usize, ws.ws_col as usize)
            } else {
                (24, 80)
            }
        }
    }

    /// Blocking single-byte read. `Err(true)` = interrupted by a resize, `Err(false)` = EOF/error.
    fn read_byte(&self) -> Result<u8, bool> {
        loop {
            let mut b = 0u8;
            let n = unsafe { libc::read(self.fd, (&mut b as *mut u8).cast(), 1) };
            if n == 1 {
                return Ok(b);
            }
            if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                if WINCH.swap(false, Ordering::Relaxed) {
                    return Err(true);
                }
                continue;
            }
            return Err(false);
        }
    }

    /// Read a byte only if one arrives within `ms` milliseconds.
    fn read_byte_timeout(&self, ms: i32) -> Option<u8> {
        let mut p = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r > 0 {
            self.read_byte().ok()
        } else {
            None
        }
    }

    pub fn key(&self) -> Key {
        let b = match self.read_byte() {
            Ok(b) => b,
            Err(true) => return Key::Resize,
            Err(false) => return Key::Eof,
        };
        match b {
            0x1b => self.escape(),
            b'\r' | b'\n' => Key::Enter,
            b'\t' => Key::Tab,
            0x7f | 0x08 => Key::Backspace,
            0x01..=0x1a => Key::Ctrl(b + 96),
            0x20..=0x7e => Key::Char(b as char),
            0xc0..=0xf7 => {
                let len = if b >= 0xf0 { 4 } else if b >= 0xe0 { 3 } else { 2 };
                let mut buf = [b, 0, 0, 0];
                for slot in buf.iter_mut().take(len).skip(1) {
                    match self.read_byte_timeout(20) {
                        Some(c) => *slot = c,
                        None => return Key::Other,
                    }
                }
                match std::str::from_utf8(&buf[..len]).ok().and_then(|s| s.chars().next()) {
                    Some(c) => Key::Char(c),
                    None => Key::Other,
                }
            }
            _ => Key::Other,
        }
    }

    fn escape(&self) -> Key {
        // A lone ESC has nothing following it; sequences arrive in one burst.
        let Some(b) = self.read_byte_timeout(25) else {
            return Key::Esc;
        };
        match b {
            b'[' => {
                let mut params = [0u8; 8];
                let mut n = 0;
                loop {
                    let Some(c) = self.read_byte_timeout(25) else {
                        return Key::Other;
                    };
                    if (0x40..=0x7e).contains(&c) {
                        return match c {
                            b'A' => Key::Up,
                            b'B' => Key::Down,
                            b'H' => Key::Home,
                            b'F' => Key::End,
                            b'~' => match params[..n].first() {
                                Some(b'1') | Some(b'7') => Key::Home,
                                Some(b'4') | Some(b'8') => Key::End,
                                Some(b'5') => Key::PageUp,
                                Some(b'6') => Key::PageDown,
                                _ => Key::Other,
                            },
                            _ => Key::Other,
                        };
                    }
                    if n < params.len() {
                        params[n] = c;
                        n += 1;
                    }
                }
            }
            b'O' => match self.read_byte_timeout(25) {
                Some(b'A') => Key::Up,
                Some(b'B') => Key::Down,
                Some(b'H') => Key::Home,
                Some(b'F') => Key::End,
                _ => Key::Other,
            },
            _ => Key::Other,
        }
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.write("\x1b[0m\x1b[?25h\x1b[?1049l");
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &self.orig);
            libc::close(self.fd);
        }
    }
}
