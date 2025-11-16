use std::cmp;
use std::io::{self, IsTerminal, Write};

/// Progress helper mirroring `Neo.CLI/CLI/ConsolePercent`.
#[derive(Debug)]
pub struct ConsolePercent {
    max_value: u64,
    value: u64,
    last_factor: f64,
    last_percent: Option<String>,
    input_redirected: bool,
}

impl ConsolePercent {
    pub fn new(value: u64, max_value: u64) -> Self {
        let mut percent = Self {
            max_value,
            value: value.min(max_value),
            last_factor: f64::NAN,
            last_percent: None,
            input_redirected: !io::stdin().is_terminal(),
        };
        percent.invalidate();
        percent
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn set_value(&mut self, value: u64) {
        let clamped = value.min(self.max_value);
        if clamped != self.value {
            self.value = clamped;
            self.invalidate();
        }
    }

    pub fn max_value(&self) -> u64 {
        self.max_value
    }

    pub fn set_max_value(&mut self, max_value: u64) {
        if max_value == self.max_value {
            return;
        }
        self.max_value = max_value;
        self.value = cmp::min(self.value, self.max_value);
        self.invalidate();
    }

    pub fn percent(&self) -> f64 {
        if self.max_value == 0 {
            0.0
        } else {
            (self.value as f64 * 100.0) / self.max_value as f64
        }
    }

    fn invalidate(&mut self) {
        let percent_value = self.percent();
        let factor = ((percent_value / 100.0) * 10.0).round() / 10.0;
        let percent_str = format!("{percent_value:>5.1}");

        if (self.last_factor - factor).abs() < f64::EPSILON
            && self.last_percent.as_deref() == Some(&percent_str)
        {
            return;
        }

        self.last_factor = factor;
        self.last_percent = Some(percent_str.clone());

        let filled = (factor * 10.0).round() as usize;
        let clamped_filled = filled.clamp(0, 10);
        let fill = "■".repeat(clamped_filled);
        let clean = if self.input_redirected {
            "□".repeat(10 - clamped_filled)
        } else {
            " ".repeat(10 - clamped_filled)
        };

        if self.input_redirected {
            println!("[{fill}{clean}] ({percent_str}%)");
        } else {
            print!("\r[{fill}{clean}] ({percent_str}%)");
            let _ = io::stdout().flush();
        }
    }
}

impl Drop for ConsolePercent {
    fn drop(&mut self) {
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_scales_with_value() {
        let mut percent = ConsolePercent::new(0, 100);
        assert_eq!(percent.percent(), 0.0);
        percent.set_value(50);
        assert_eq!(percent.percent(), 50.0);
        percent.set_value(150);
        assert_eq!(percent.percent(), 100.0);
    }

    #[test]
    fn max_value_updates_clamp_value() {
        let mut percent = ConsolePercent::new(50, 100);
        percent.set_max_value(25);
        assert_eq!(percent.value(), 25);
        assert_eq!(percent.percent(), 100.0);
    }
}
