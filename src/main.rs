mod tmux;
mod tty;
mod ui;

use std::path::Path;
use std::process::ExitCode;

const HELP: &str = "\
ts — fast tmux session switcher

USAGE
  ts            list sessions, pick one to switch/attach   (also: n = new session)
  tn            create a new session (asks for a name)
  tk            kill sessions (multi-select)

The three commands are one binary (argv[0] picks the mode). You can also
use subcommands:  ts new | ts kill

KEYS   ↑↓ / j k / ctrl-n ctrl-p move   ⏎ select   1-9 jump   esc/q quit
";

#[derive(PartialEq)]
enum Mode {
    List,
    New,
    Kill,
}

fn default_name() -> String {
    let cwd = std::env::current_dir().ok();
    let base = cwd
        .as_deref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(tmux::sanitize)
        .unwrap_or_default();
    if base.is_empty() {
        "main".into()
    } else {
        base
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_default();
    let prog = Path::new(&argv0).file_name().and_then(|s| s.to_str()).unwrap_or("ts");
    let mut mode = match prog {
        "tn" => Mode::New,
        "tk" => Mode::Kill,
        _ => Mode::List,
    };
    if let Some(a) = args.next() {
        mode = match a.as_str() {
            "new" | "n" => Mode::New,
            "kill" | "k" | "rm" | "del" => Mode::Kill,
            "ls" | "list" | "l" => Mode::List,
            "-h" | "--help" | "help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "-V" | "--version" => {
                println!("ts {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("ts: unknown argument '{other}' (try --help)");
                return ExitCode::from(2);
            }
        };
    }

    match run(mode) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("ts: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(mode: Mode) -> Result<(), String> {
    let (sessions, cur_id) = tmux::list().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "tmux not found in PATH".to_string(),
        _ => format!("failed to run tmux: {e}"),
    })?;

    if mode == Mode::Kill && sessions.is_empty() {
        println!("no tmux sessions");
        return Ok(());
    }

    let default = default_name();
    let cur = cur_id.as_deref();

    // The Tty lives only inside this block, so the terminal is fully restored
    // before we exec tmux or print anything.
    let outcome = {
        let tty = tty::Tty::open().map_err(|e| format!("cannot open /dev/tty: {e}"))?;
        match mode {
            Mode::List => ui::list(&tty, &sessions, cur, &default),
            Mode::New => match ui::new_prompt(&tty, &sessions, &default) {
                Some(n) => ui::Outcome::Create(n),
                None => ui::Outcome::Quit,
            },
            Mode::Kill => match ui::kill_menu(&tty, &sessions, cur) {
                Some(v) => ui::Outcome::Kill(v),
                None => ui::Outcome::Quit,
            },
        }
    };

    match outcome {
        ui::Outcome::Quit => Ok(()),
        ui::Outcome::Open(name) => Err(format!("exec tmux failed: {}", tmux::open(&name))),
        ui::Outcome::Create(name) => {
            let exists = sessions.iter().any(|s| s.name == name);
            Err(format!("exec tmux failed: {}", tmux::create(&name, exists)))
        }
        ui::Outcome::Kill(names) => {
            let cur_name = sessions.iter().find(|s| Some(s.id.as_str()) == cur).map(|s| s.name.as_str());
            let mut failed = false;
            for (name, ok) in tmux::kill(&names, cur_name) {
                if ok {
                    println!("killed {name}");
                } else {
                    eprintln!("failed to kill {name}");
                    failed = true;
                }
            }
            if failed {
                Err("some sessions could not be killed".into())
            } else {
                Ok(())
            }
        }
    }
}
