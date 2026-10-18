use regex::{Captures, Regex};
use std::{collections::BTreeMap, sync::OnceLock};
pub struct Redactor {
    known: Vec<(String, String)>,
    values: BTreeMap<String, String>,
}
impl Redactor {
    pub fn new(identifiers: Vec<String>) -> Self {
        let mut known = vec![];
        if let Ok(home) = std::env::var("USERPROFILE") {
            known.push((home.clone(), "[USER_HOME]".into()));
            known.push((home.replace('\\', "\\\\"), "[USER_HOME]".into()));
        }
        for (i, id) in identifiers.into_iter().filter(|s| s.len() >= 4).enumerate() {
            known.push((id, format!("[DEVICE_{}]", i + 1)));
        }
        known.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));
        Self {
            known,
            values: BTreeMap::new(),
        }
    }
    fn value(&mut self, s: &str) -> String {
        let n = self.values.len() + 1;
        self.values
            .entry(s.into())
            .or_insert_with(|| format!("[IDENTIFIER_{n}]"))
            .clone()
    }
    pub fn redact(&mut self, line: &str) -> String {
        let mut text = line.to_string();
        for (k, v) in &self.known {
            text = text.replace(k, v);
        }
        static HOME: OnceLock<Regex> = OnceLock::new();
        text = HOME
            .get_or_init(|| {
                Regex::new(r"(?i)[A-Z]:[\\/]+Users[\\/]+[^\\/\s\x22]+|/home/[^/\s\x22]+").unwrap()
            })
            .replace_all(&text, "[USER_HOME]")
            .into();
        static IP: OnceLock<Regex> = OnceLock::new();
        let regex = IP.get_or_init(|| {
            Regex::new(r"\b(?:\d{1,3}\.){3}\d{1,3}\b|(?:[A-Fa-f0-9]{0,4}:){2,}[A-Fa-f0-9]{0,4}")
                .unwrap()
        });
        text = regex
            .replace_all(&text, |c: &Captures| {
                let s = &c[0];
                if s.parse::<std::net::IpAddr>().is_ok() {
                    self.value(s)
                } else {
                    s.into()
                }
            })
            .into();
        // Authentication values contain spaces (scheme + credentials). Redact the
        // whole value before the generic single-token field rule can split it.
        static AUTHORIZATION: OnceLock<Regex> = OnceLock::new();
        text = AUTHORIZATION
            .get_or_init(|| {
                Regex::new(r#"(?i)\b((?:proxy-)?authorization)(["']?[ \t]*[:=][ \t]*)("(?:\\.|[^"\\\r\n])*"|'(?:\\.|[^'\\\r\n])*'|[^"'\r\n,;}]+)"#).unwrap()
            })
            .replace_all(&text, |c: &Captures| {
                let quote = c[3].chars().next().filter(|q| matches!(q, '"' | '\''));
                match quote {
                    Some(q) => format!("{}{}{q}[REDACTED]{q}", &c[1], &c[2]),
                    None => format!("{}{}[REDACTED]", &c[1], &c[2]),
                }
            })
            .into();
        static BEARER: OnceLock<Regex> = OnceLock::new();
        text = BEARER
            .get_or_init(|| Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+").unwrap())
            .replace_all(&text, "Bearer [REDACTED]")
            .into();
        static SECRET: OnceLock<Regex> = OnceLock::new();
        text=SECRET.get_or_init(||Regex::new(r#"(?i)(password|passwd|token|secret|cookie|pair(?:ing)?[_ -]?code)(["'\s]*[:=]["'\s]*)([^\s,;"']+)"#).unwrap()).replace_all(&text,"$1$2[REDACTED]").into();
        text
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_entire_http_and_json_authorization_values() {
        let mut r = Redactor::new(vec![]);
        for input in [
            "Authorization: Bearer review-credential",
            "authorization: Basic review-credential",
            "Proxy-Authorization: Basic review-credential",
            r#"{"Authorization": "Bearer review-credential", "other": "kept"}"#,
            r#"{"authorization":"Basic review-credential"}"#,
            "'Authorization' = 'Bearer review-credential'",
        ] {
            let output = r.redact(input);
            assert!(!output.contains("review-credential"), "{output}");
            assert!(output.contains("[REDACTED]"));
        }
        let json = r.redact(r#"{"Authorization": "Bearer credential", "other": "kept"}"#);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["Authorization"], "[REDACTED]");
        assert_eq!(parsed["other"], "kept");
    }
    #[test]
    fn stable_placeholders_and_secret_fields() {
        let mut r = Redactor::new(vec!["SERIAL123".into()]);
        let a = r.redact(r#"C:\Users\alice\work SERIAL123 192.168.1.8 token=abc password: xyz"#);
        assert!(
            !a.contains("alice")
                && !a.contains("SERIAL123")
                && !a.contains("192.168.1.8")
                && !a.contains("abc")
                && !a.contains("xyz")
        );
        let b = r.redact("192.168.1.8");
        assert!(a.contains(&b));
        assert!(!r.redact("Bearer eyAAA.secret").contains("eyAAA"));
    }
}
