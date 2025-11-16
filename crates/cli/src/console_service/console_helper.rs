use super::color_set::ConsoleColorSet;
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    style::Color,
    terminal,
};
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// Console helper utilities mirroring `Neo.ConsoleService.ConsoleHelper`.
pub struct ConsoleHelper;

static READING_PASSWORD: AtomicBool = AtomicBool::new(false);

impl ConsoleHelper {
    pub fn info(parts: impl IntoIterator<Item = impl AsRef<str>>) {
        log_with_pairs(
            parts,
            ConsoleColorSet::foreground(Color::Cyan),
            ConsoleColorSet::default(),
        );
    }

    pub fn warning(message: impl AsRef<str>) {
        log_single(
            "Warning",
            ConsoleColorSet::foreground(Color::Yellow),
            message,
        );
    }

    pub fn error(message: impl AsRef<str>) {
        log_single("Error", ConsoleColorSet::foreground(Color::Red), message);
    }

    pub fn is_reading_password() -> bool {
        READING_PASSWORD.load(Ordering::SeqCst)
    }

    pub fn read_user_input(prompt: &str, password: bool) -> Result<String> {
        let mut stdout = io::stdout();
        if !prompt.is_empty() {
            write!(stdout, "{}: ", prompt)?;
            stdout.flush()?;
        }

        if password {
            READING_PASSWORD.store(true, Ordering::SeqCst);
        }
        let mut password_guard = PasswordGuard(password);

        ConsoleColorSet::foreground(Color::Yellow)
            .apply_to(&mut stdout)
            .ok();

        let interactive = io::stdin().is_terminal();
        let mut result = if password && interactive {
            read_password_interactive(&mut stdout)?
        } else {
            read_line_from_stdin()?
        };

        // Ensure carriage returns are stripped to align with the C# helper.
        trim_newline(&mut result);

        ConsoleColorSet::default().apply_to(&mut stdout).ok();
        writeln!(stdout)?;
        stdout.flush()?;

        password_guard.done();
        Ok(result)
    }
}

fn log_with_pairs(
    parts: impl IntoIterator<Item = impl AsRef<str>>,
    tag_color: ConsoleColorSet,
    value_color: ConsoleColorSet,
) {
    let mut stdout = io::stdout();
    for (index, part) in parts.into_iter().enumerate() {
        if index % 2 == 0 {
            tag_color.apply_to(&mut stdout).ok();
        } else {
            value_color.apply_to(&mut stdout).ok();
        }
        write!(stdout, "{}", part.as_ref()).ok();
    }
    value_color.apply_to(&mut stdout).ok();
    writeln!(stdout).ok();
    stdout.flush().ok();
}

fn log_single(tag: &str, color: ConsoleColorSet, message: impl AsRef<str>) {
    let mut stdout = io::stdout();
    color.apply_to(&mut stdout).ok();
    write!(stdout, "{tag}: ").ok();
    ConsoleColorSet::default().apply_to(&mut stdout).ok();
    writeln!(stdout, "{}", message.as_ref()).ok();
    stdout.flush().ok();
}

fn read_line_from_stdin() -> Result<String> {
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .context("failed to read from stdin")?;
    Ok(line)
}

fn read_password_interactive(stdout: &mut io::Stdout) -> Result<String> {
    let _guard = RawModeGuard::new()?;
    let mut buffer = String::new();
    loop {
        match event::read().context("failed to read console event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => break,
                KeyCode::Backspace => {
                    if buffer.pop().is_some() {
                        write!(stdout, "\u{8} \u{8}")?;
                        stdout.flush()?;
                    }
                }
                KeyCode::Char(ch) if is_printable_ascii(ch) => {
                    buffer.push(ch);
                    write!(stdout, "*")?;
                    stdout.flush()?;
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(buffer)
}

fn trim_newline(value: &mut String) {
    while value.ends_with(['\n', '\r']) {
        value.pop();
    }
}

fn is_printable_ascii(ch: char) -> bool {
    ch == ' ' || ch.is_ascii_graphic()
}

struct PasswordGuard(bool);

impl PasswordGuard {
    fn done(&mut self) {
        if self.0 {
            READING_PASSWORD.store(false, Ordering::SeqCst);
            self.0 = false;
        }
    }
}

impl Drop for PasswordGuard {
    fn drop(&mut self) {
        if self.0 {
            READING_PASSWORD.store(false, Ordering::SeqCst);
        }
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn new() -> Result<Self> {
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_ascii_matches_range() {
        assert!(is_printable_ascii('A'));
        assert!(is_printable_ascii(' '));
        assert!(!is_printable_ascii('\t'));
    }

    #[test]
    fn trim_newline_removes_crlf() {
        let mut value = "hello\r\n".to_string();
        trim_newline(&mut value);
        assert_eq!(value, "hello");
    }
}
