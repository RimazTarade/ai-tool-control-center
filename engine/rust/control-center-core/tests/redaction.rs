use control_center_core::redaction::redact;

#[test]
fn redact_hides_common_secret_values() {
    let input =
        "Authorization: Bearer super-secret-token api_key=abc123 password=hunter2 token=xyz789";

    let redacted = redact(input);

    assert!(!redacted.contains("super-secret-token"));
    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("hunter2"));
    assert!(!redacted.contains("xyz789"));

    assert!(redacted.contains("[REDACTED]"));
}

#[test]
fn redact_hides_json_secret_values() {
    let input = r#"{"api_key":"abc123","token":"xyz789","password":"hunter2"}"#;

    let redacted = redact(input);

    assert!(!redacted.contains("abc123"));
    assert!(!redacted.contains("xyz789"));
    assert!(!redacted.contains("hunter2"));
    assert!(redacted.contains("[REDACTED]"));
}
