use log::Level;

/// Structured log event: one line of `event="action" key="value"` pairs
/// emitted under a logical scope (the `log` target). Scopes use dotted
/// lowercase names like `terminal.connection_service`, matching the
/// frontend logger scopes (`frontend.*`).
#[derive(Clone, Debug)]
pub(crate) struct LogEvent {
    scope: &'static str,
    action: String,
    fields: Vec<(&'static str, String)>,
}

pub(crate) fn event(scope: &'static str, action: impl Into<String>) -> LogEvent {
    LogEvent {
        scope,
        action: action.into(),
        fields: Vec::new(),
    }
}

impl LogEvent {
    pub(crate) fn field(mut self, key: &'static str, value: impl ToString) -> Self {
        self.fields.push((key, sanitize_log_value(value)));
        self
    }

    pub(crate) fn maybe_field(mut self, key: &'static str, value: Option<impl ToString>) -> Self {
        if let Some(value) = value {
            self.fields.push((key, sanitize_log_value(value)));
        }
        self
    }

    pub(crate) fn debug(self) {
        self.emit(Level::Debug);
    }

    pub(crate) fn info(self) {
        self.emit(Level::Info);
    }

    pub(crate) fn warn(self) {
        self.emit(Level::Warn);
    }

    pub(crate) fn error(self) {
        self.emit(Level::Error);
    }

    pub(crate) fn trace(self) {
        self.emit(Level::Trace);
    }

    fn emit(self, level: Level) {
        let mut line = format!("event=\"{}\"", escape_log_value(&self.action));
        for (key, value) in self.fields {
            line.push(' ');
            line.push_str(key);
            line.push('=');
            line.push('"');
            line.push_str(&escape_log_value(&value));
            line.push('"');
        }
        log::log!(target: self.scope, level, "{line}");
    }
}

/// Newlines/tabs would break the one-event-per-line contract or inject fake
/// log lines, so they are flattened to spaces.
fn sanitize_log_value(value: impl ToString) -> String {
    value
        .to_string()
        .replace(['\r', '\n', '\t'], " ")
        .trim()
        .to_string()
}

fn escape_log_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_flattens_control_characters() {
        assert_eq!(sanitize_log_value("a\r\nb\tc"), "a  b c");
    }

    #[test]
    fn escape_quotes_and_backslashes() {
        assert_eq!(escape_log_value("a\"b\\c"), "a\\\"b\\\\c");
    }
}
