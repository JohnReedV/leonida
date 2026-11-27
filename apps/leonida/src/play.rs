//! What `leonida` does with no command, as when leonida.exe is double-clicked:
//! what `run-gta6.ps1` does after its build step, for a player who has never
//! opened a terminal. The console window stays open until they have read how
//! it ended.
//!
//! The warm-up and the game run as `leonida translate --warm all` and
//! `leonida run --execute` in this window, so they take the same paths as
//! from the script, a title that relaunches itself gets a command line it can
//! use again, and a crash still leaves this process to say what happened.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use anyhow::{Context, Result};

const WARM_UP_QUESTION: &str = "\
Warm up the shader cache first? It translates every shader before the game starts:
about 1 to 2 minutes the first time (and after an update), about 30 s when the cache is full.
WARNING: without it, the game translates shaders while you play, so it stutters and runs
slower until the cache fills up.";

/// Never returns: waits for Enter, then ends with the game's exit code (1 if
/// it could not be started).
pub fn run() -> ! {
    println!("{}", title(crate::BUILD));
    let code = match play() {
        Ok(status) if status.success() => {
            println!();
            println!("The game has closed.");
            0
        }
        Ok(status) => {
            println!();
            println!("The game ended with {}.", describe(status));
            println!("If it did not end on purpose, the lines above say why.");
            if let Ok(cfg) = leonida_core::Config::load() {
                println!(
                    "A crash also leaves a report in crashes.log under {}.",
                    cfg.data_dir.display()
                );
            }
            println!("Start leonida.exe again to retry.");
            status.code().unwrap_or(1)
        }
        Err(e) => {
            println!();
            println!("Could not start the game: {e:#}");
            println!("Fix that, then start leonida.exe again.");
            1
        }
    };
    wait_for_enter();
    std::process::exit(code)
}

fn play() -> Result<ExitStatus> {
    let dir = game_dir()?;

    // Shaders not in the cache are translated the first time the game uses
    // them, mid-play.
    println!();
    println!("{WARM_UP_QUESTION}");
    if ask_yes("Warm up now?") {
        match leonida(&["translate", "--warm", "all"], &dir, false) {
            Ok(status) if status.success() => {}
            Ok(status) => {
                println!(
                    "WARNING: the warm-up failed ({}); starting anyway",
                    describe(status)
                )
            }
            Err(e) => println!("WARNING: the warm-up did not start ({e:#}); starting anyway"),
        }
    } else {
        println!(
            "WARNING: starting without a warm-up: expect stutter until the shader cache fills up"
        );
    }

    println!();
    println!("Starting the game. Keep this window open: it shows warnings and errors.");
    leonida(&["run", "--execute"], &dir, true)
}

/// The configured game folder if it has a MicrosoftGame.config, else the
/// one the player gives (saved to leonida.toml).
fn game_dir() -> Result<PathBuf> {
    let cfg = leonida_core::Config::load().context(
        "leonida.toml in this folder could not be read; correct it, or delete it to be \
         asked for the game folder again",
    )?;
    if let Some(dir) = &cfg.game_dir {
        if dir.join("MicrosoftGame.config").is_file() {
            println!("Game folder: {}", dir.display());
            return Ok(dir.clone());
        }
        println!(
            "The saved game folder has no MicrosoftGame.config: {}",
            dir.display()
        );
    }
    println!(
        "Enter the folder of your copy of the game (the one with MicrosoftGame.config in it)."
    );
    println!("You can drag the folder from File Explorer into this window, then press Enter:");
    crate::read_game_dir()
}

/// Runs this program again in this window with `args`, for the game in
/// `game_dir`. `quiet` keeps the console to warnings and errors unless
/// `LEONIDA_LOG` asks for more (`LEONIDA_LOG=info` shows the GPU layer's
/// status lines).
fn leonida(args: &[&str], game_dir: &Path, quiet: bool) -> Result<ExitStatus> {
    let exe = std::env::current_exe().context("finding leonida.exe")?;
    let mut command = std::process::Command::new(exe);
    command.args(args).env("LEONIDA_GAME_DIR", game_dir);
    if quiet && std::env::var_os("LEONIDA_LOG").is_none_or(|v| v.is_empty()) {
        command.env("LEONIDA_LOG", "warn");
    }
    command
        .status()
        .with_context(|| format!("starting leonida {}", args.join(" ")))
}

fn title(build: &str) -> String {
    if build == "local" {
        "Leonida Legacy (local build)".to_owned()
    } else {
        format!("Leonida Legacy, build {build}")
    }
}

/// An exit code; a Windows exception code (negative as an `i32`) in hex.
fn describe(status: ExitStatus) -> String {
    match status.code() {
        Some(code) if code < 0 => format!("exit code {:#010x}", code as u32),
        Some(code) => format!("exit code {code}"),
        None => status.to_string(),
    }
}

/// Asks a question whose default is yes. A closed input counts as Enter.
fn ask_yes(question: &str) -> bool {
    print!("{question} [Y/n]: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    is_yes(&line)
}

fn is_yes(answer: &str) -> bool {
    matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

fn wait_for_enter() {
    println!();
    print!("Press Enter to close this window.");
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().lock().read_line(&mut String::new());
}

#[cfg(test)]
mod tests {
    use super::{is_yes, title};

    #[test]
    fn enter_and_yes_mean_yes() {
        for answer in ["", "\r\n", "y", "Y\r\n", " yes ", "YES"] {
            assert!(is_yes(answer), "{answer:?}");
        }
        for answer in ["n", "no\r\n", "N", "yep", "x"] {
            assert!(!is_yes(answer), "{answer:?}");
        }
    }

    #[test]
    fn the_title_names_the_build() {
        assert_eq!(title("123"), "Leonida Legacy, build 123");
        assert_eq!(title("local"), "Leonida Legacy (local build)");
    }
}
