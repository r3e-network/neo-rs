//! Operator-facing log format parsing.

#[derive(Clone, Copy)]
pub(super) enum LogFormat {
    Pretty,
    Compact,
    Json,
}

pub(super) fn log_format(format: Option<&str>) -> anyhow::Result<LogFormat> {
    match format
        .unwrap_or("pretty")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "pretty" => Ok(LogFormat::Pretty),
        "compact" => Ok(LogFormat::Compact),
        "json" => Ok(LogFormat::Json),
        other => {
            anyhow::bail!(
                "unsupported [logging].format {other:?}; expected pretty, compact, or json"
            );
        }
    }
}
