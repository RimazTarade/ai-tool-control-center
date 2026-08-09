from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from ai_tool_control_scanner.protocol import ProtocolError, decode, redact
from ai_tool_control_scanner.scanners import scan


class ScannerTest(unittest.TestCase):
    def test_protocol_rejects_malformed_and_redacts(self) -> None:
        with self.assertRaises(ProtocolError):
            decode("not-json")
        self.assertEqual(redact("token=very-secret-value"), "token=[REDACTED]")

    def test_mcp_config_is_evidence_not_health(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "example-mcp.json"
            path.write_text('{"mcpServers": {"example": {}}}', encoding="utf-8")
            [result] = list(scan((directory,)))
            self.assertEqual(result["suggested_type"], "mcp")
            self.assertEqual(result["health_state"], "unknown")


if __name__ == "__main__":
    unittest.main()
