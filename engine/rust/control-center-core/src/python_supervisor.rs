use serde::Deserialize;

use crate::{Confidence, Discovery, Evidence, ToolKind};

const SCANNER_PROTOCOL: &str = "scanner_protocol";

#[derive(Debug, Deserialize)]
struct PythonResponse {
    protocol_version: u64,
    kind: String,
    discovery: Option<PythonDiscovery>,
    count: Option<u64>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PythonDiscovery {
    fingerprint: String,
    suggested_name: String,
    suggested_type: String,
    confidence: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PythonScannerEvent {
    Discovery(Discovery),
    Completed {
        count: u64,
    },
    Pong,
    Cancelled,
    Error {
        code: String,
        message: Option<String>,
    },
}

pub fn parse_python_response(line: &str) -> Result<PythonScannerEvent, &'static str> {
    let response: PythonResponse = serde_json::from_str(line).map_err(|_| SCANNER_PROTOCOL)?;

    if response.protocol_version != 1 {
        return Err(SCANNER_PROTOCOL);
    }

    match response.kind.as_str() {
        "discovery" => translate_discovery_response(line).map(PythonScannerEvent::Discovery),
        "pong" => Ok(PythonScannerEvent::Pong),
        "cancelled" => Ok(PythonScannerEvent::Cancelled),
        "error" => Ok(PythonScannerEvent::Error {
            code: response.code.ok_or(SCANNER_PROTOCOL)?,
            message: response.message,
        }),
        "completed" => Ok(PythonScannerEvent::Completed {
            count: response.count.ok_or(SCANNER_PROTOCOL)?,
        }),
        _ => Err(SCANNER_PROTOCOL),
    }
}
fn translate_discovery_response(line: &str) -> Result<Discovery, &'static str> {
    let response: PythonResponse = serde_json::from_str(line).map_err(|_| SCANNER_PROTOCOL)?;

    if response.protocol_version != 1 || response.kind != "discovery" {
        return Err(SCANNER_PROTOCOL);
    }

    let python = response.discovery.ok_or(SCANNER_PROTOCOL)?;

    let suggested_type = match python.suggested_type.as_str() {
        "mcp" => ToolKind::Mcp,
        "claude" | "codex" | "docker" | "ollama" => ToolKind::Configuration,
        "unknown" => ToolKind::Unknown,
        _ => return Err(SCANNER_PROTOCOL),
    };

    let confidence = match python.confidence.as_str() {
        "low" => Confidence::Low,
        "medium" => Confidence::Medium,
        "high" => Confidence::High,
        _ => return Err(SCANNER_PROTOCOL),
    };

    let mut discovery =
        Discovery::unknown(python.suggested_name, "python.config", python.fingerprint);
    discovery.suggested_type = suggested_type;
    discovery.confidence = confidence;
    discovery.evidence = python.evidence;

    Ok(discovery)
}

#[cfg(test)]
mod tests {
    use crate::{Confidence, ObservedState, ToolKind};

    use super::{PythonScannerEvent, parse_python_response, translate_discovery_response};

    #[test]
    fn translates_mcp_discovery_into_rust_domain_model() {
        let line = r#"{
            "protocol_version":1,"request_id":"req-1",
            "kind":"discovery",
            "discovery":{
                "fingerprint":"abc123",
                "suggested_name":"config",
                "suggested_type":"mcp",
                "confidence":"medium",
                "evidence":[
                    {"kind":"path","summary":"C:\\Users\\test\\.mcp\\config.json"},
                    {"kind":"reason","summary":"configuration contains an MCP server mapping"}
                ],
                "health_state":"unknown"
            }
        }"#;

        let discovery = translate_discovery_response(line).unwrap();

        assert_eq!(discovery.fingerprint, "abc123");
        assert_eq!(discovery.suggested_name, "config");
        assert_eq!(discovery.suggested_type, ToolKind::Mcp);
        assert_eq!(discovery.source_scanner, "python.config");
        assert_eq!(discovery.confidence, Confidence::Medium);
        assert_eq!(discovery.evidence.len(), 2);

        assert_eq!(discovery.installation_state, ObservedState::Detected);
        assert_eq!(discovery.registration_state, ObservedState::Unknown);
        assert_eq!(discovery.enablement_state, ObservedState::Unknown);
        assert_eq!(discovery.runtime_state, ObservedState::Unknown);
        assert_eq!(discovery.connection_state, ObservedState::Unknown);
        assert_eq!(discovery.authentication_state, ObservedState::Unknown);
        assert_eq!(discovery.health_state, ObservedState::Unknown);
    }

    #[test]
    fn rejects_unrecognized_python_tool_kind() {
        let line = r#"{
            "request_id":"req-2",
            "kind":"discovery",
            "discovery":{
                "fingerprint":"def456",
                "suggested_name":"mystery",
                "suggested_type":"invented_kind",
                "confidence":"low",
                "evidence":[],
                "health_state":"unknown"
            }
        }"#;

        assert_eq!(translate_discovery_response(line), Err("scanner_protocol"));
    }

    #[test]
    fn maps_remaining_python_product_labels() {
        for (label, expected) in [
            ("claude", ToolKind::Configuration),
            ("codex", ToolKind::Configuration),
            ("docker", ToolKind::Configuration),
            ("ollama", ToolKind::Configuration),
            ("unknown", ToolKind::Unknown),
        ] {
            let line = format!(
                r#"{{"protocol_version":1,"kind":"discovery","discovery":{{"fingerprint":"abc","suggested_name":"config","suggested_type":"{label}","confidence":"low","evidence":[]}}}}"#
            );
            let discovery = translate_discovery_response(&line).unwrap();
            assert_eq!(discovery.suggested_type, expected);
        }
    }

    #[test]
    fn rejects_wrong_python_protocol_version() {
        let line = r#"{"protocol_version":2,"kind":"discovery","discovery":{"fingerprint":"abc","suggested_name":"config","suggested_type":"mcp","confidence":"low","evidence":[]}}"#;

        assert_eq!(translate_discovery_response(line), Err("scanner_protocol"));
    }

    #[test]
    fn parses_completed_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-3","kind":"completed","count":4}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Completed { count: 4 })
        );
    }

    #[test]
    fn parses_pong_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-ping","kind":"pong"}"#;

        assert_eq!(parse_python_response(line), Ok(PythonScannerEvent::Pong));
    }

    #[test]
    fn parses_cancelled_response() {
        let line = r#"{"protocol_version":1,"request_id":"req-cancel","kind":"cancelled"}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Cancelled)
        );
    }

    #[test]
    fn parses_error_response() {
        let line = r#"{"protocol_version":1,"kind":"error","code":"scanner_failed","message":"safe message"}"#;

        assert_eq!(
            parse_python_response(line),
            Ok(PythonScannerEvent::Error {
                code: "scanner_failed".to_string(),
                message: Some("safe message".to_string()),
            })
        );
    }

    #[test]
    fn rejects_malformed_python_json() {
        assert_eq!(parse_python_response("{not-json"), Err("scanner_protocol"));
    }
}
