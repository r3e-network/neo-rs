use crossterm::{
    execute,
    style::{Color, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, Stdout, Write};

/// Simple color wrapper mirroring `Neo.ConsoleService.ConsoleColorSet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsoleColorSet {
    foreground: Option<Color>,
    background: Option<Color>,
}

impl ConsoleColorSet {
    pub fn new(foreground: Option<Color>, background: Option<Color>) -> Self {
        Self {
            foreground,
            background,
        }
    }

    pub fn foreground(color: Color) -> Self {
        Self::new(Some(color), None)
    }

    pub fn background(color: Color) -> Self {
        Self::new(None, Some(color))
    }

    pub fn with_colors(foreground: Color, background: Color) -> Self {
        Self::new(Some(foreground), Some(background))
    }

    /// Applies the stored colors to the global stdout handle.
    pub fn apply(&self) {
        let mut stdout = io::stdout();
        let _ = self.apply_to(&mut stdout);
    }

    /// Applies the stored colors to a specific writer.
    pub fn apply_to(&self, stdout: &mut Stdout) -> io::Result<()> {
        execute!(
            stdout,
            SetForegroundColor(self.foreground.unwrap_or(Color::Reset)),
            SetBackgroundColor(self.background.unwrap_or(Color::Reset))
        )?;
        stdout.flush()?;
        Ok(())
    }
}

impl Default for ConsoleColorSet {
    fn default() -> Self {
        Self::new(None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_color_set_has_no_colors() {
        let set = ConsoleColorSet::default();
        assert_eq!(set.foreground, None);
        assert_eq!(set.background, None);
    }

    #[test]
    fn custom_color_set_stores_values() {
        let set = ConsoleColorSet::with_colors(Color::Blue, Color::White);
        assert_eq!(set.foreground, Some(Color::Blue));
        assert_eq!(set.background, Some(Color::White));
    }
}
