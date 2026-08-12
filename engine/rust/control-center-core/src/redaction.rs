use regex::Regex;

pub fn redact(input: &str) -> String {
    let bearer = Regex::new(r"(?i)(Authorization:\s*Bearer\s+)\S+").unwrap();
    let keyed = Regex::new(r"(?i)\b(api[_-]?key|token|password)\s*=\s*[^\s&;,]+").unwrap();
    let json_keyed =
        Regex::new(r#"(?i)("(?:api[_-]?key|token|password)"\s*:\s*")[^"]*(")"#).unwrap();

    let value = bearer.replace_all(input, "${1}[REDACTED]");
    let value = keyed.replace_all(&value, "${1}=[REDACTED]");

    json_keyed
        .replace_all(&value, "${1}[REDACTED]${2}")
        .into_owned()
}
