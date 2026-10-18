use crate::domain::{LogEntry, LogFilter};
use std::collections::VecDeque;
pub fn parse(raw: &str, seq: u64) -> LogEntry {
    let mut row = LogEntry {
        seq,
        raw: raw.chars().take(8192).collect(),
        ..Default::default()
    };
    if raw.chars().nth(8192).is_some() {
        row.raw.push_str(" [单行截断]");
    }
    let tokens: Vec<_> = row.raw.split_whitespace().take(5).collect();
    if tokens.len() == 5
        && tokens[0].contains('-')
        && tokens[1].contains(':')
        && tokens[2].parse::<u32>().is_ok()
        && tokens[3].parse::<u32>().is_ok()
        && "VDIWEFAS".contains(tokens[4])
        && tokens[4].len() == 1
    {
        row.timestamp = format!("{} {}", tokens[0], tokens[1]);
        row.pid = tokens[2].parse().unwrap_or(0);
        row.tid = tokens[3].parse().unwrap_or(0);
        row.level = tokens[4].into();
        let mut rest = row.raw.as_str();
        for _ in 0..5 {
            rest = rest.trim_start();
            rest = rest
                .find(char::is_whitespace)
                .map(|p| &rest[p..])
                .unwrap_or("");
        }
        let (tag, text) = rest
            .trim_start()
            .split_once(':')
            .unwrap_or(("", rest.trim()));
        row.tag = tag.trim().into();
        row.text = text.trim_start().into();
    } else {
        row.level = "MARK".into();
        row.text = row.raw.clone();
    }
    row
}
pub fn matches(row: &LogEntry, f: &LogFilter, app_pids: &[u32]) -> bool {
    if row.seq <= f.after || (f.before > 0 && row.seq > f.before) {
        return false;
    }
    if row.level == "MARK" {
        return true;
    }
    let rank = |s: &str| "VDIWEFA".find(s).unwrap_or(0);
    (f.level.is_empty() || rank(&row.level) >= rank(&f.level))
        && (f.tag.is_empty() || row.tag.to_lowercase().contains(&f.tag.to_lowercase()))
        && (f.keyword.is_empty() || row.raw.to_lowercase().contains(&f.keyword.to_lowercase()))
        && (f.pids.is_empty() || f.pids.contains(&row.pid))
        && (f.package.is_empty() || app_pids.contains(&row.pid))
}
#[derive(Default)]
pub struct LogBuffer {
    pub lines: VecDeque<LogEntry>,
    pub bytes: usize,
    pub dropped: u64,
    pub latest: u64,
    pub status: String,
    pub last_used: Option<std::time::Instant>,
}
impl LogBuffer {
    pub fn push(&mut self, line: &str) {
        self.last_used = Some(std::time::Instant::now());
        self.latest += 1;
        let v = parse(line, self.latest);
        self.bytes += v.raw.len();
        self.lines.push_back(v);
        while self.lines.len() > 10_000 || self.bytes > 8 * 1024 * 1024 {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.raw.len();
                self.dropped += 1;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_filters_restarted_processes() {
        let a = parse("10-18 01:23:45.123   221   298 W MyApp   : hello: world", 1);
        assert_eq!(
            (
                a.pid,
                a.tid,
                a.level.as_str(),
                a.tag.as_str(),
                a.text.as_str()
            ),
            (221, 298, "W", "MyApp", "hello: world")
        );
        let f = LogFilter {
            package: "dev.app".into(),
            level: "W".into(),
            ..Default::default()
        };
        assert!(matches(&a, &f, &[221, 222]));
        assert!(!matches(&a, &f, &[333]));
        assert!(!matches(
            &a,
            &LogFilter {
                after: 1,
                ..Default::default()
            },
            &[]
        ));
    }
    #[test]
    fn respects_line_and_byte_limits() {
        let mut b = LogBuffer::default();
        for _ in 0..10001 {
            b.push("hello");
        }
        assert_eq!(b.lines.len(), 10000);
        assert_eq!(b.dropped, 1);
        for _ in 0..2000 {
            b.push(&"中".repeat(8192));
        }
        assert!(b.bytes <= 8 * 1024 * 1024);
        assert!(b.dropped > 1000);
        assert_eq!(b.latest, 12001);
    }
}
