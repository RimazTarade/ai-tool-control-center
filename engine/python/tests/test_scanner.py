from __future__ import annotations

import io
import tempfile
import unittest
from unittest.mock import patch
from pathlib import Path

from ai_tool_control_scanner.__main__ import main
from ai_tool_control_scanner.protocol import MAX_MESSAGE_BYTES, ProtocolError, decode, read_bounded_line, redact, write
from ai_tool_control_scanner.scanners import scan


class ScannerTest(unittest.TestCase):
    def test_protocol_rejects_malformed_and_redacts(self) -> None:
        with self.assertRaises(ProtocolError):
            decode("not-json")
        self.assertEqual(redact("token=very-secret-value"), "token=[REDACTED]")

    def test_oversized_input_is_bounded_and_next_message_survives(self) -> None:
        oversized = b"x" * (MAX_MESSAGE_BYTES + 32) + b"\n"
        valid = (
            b'{"protocol_version":1,"request_id":"ping-1","operation":"ping","roots":[]}\n'
        )
        stream = io.BytesIO(oversized + valid)

        with self.assertRaisesRegex(ProtocolError, "message_too_large"):
            read_bounded_line(stream)

        line = read_bounded_line(stream)
        self.assertIsNotNone(line)
        request = decode(line or "")
        self.assertEqual(request.request_id, "ping-1")
        self.assertEqual(request.operation, "ping")

    def test_supervisor_uses_bounded_binary_input_and_recovers(self) -> None:
        oversized = b"x" * (MAX_MESSAGE_BYTES + 32) + b"\n"
        valid = (
            b'{"protocol_version":1,"request_id":"ping-1","operation":"ping","roots":[]}\n'
        )

        class BufferOnlyStdin:
            def __init__(self, data: bytes) -> None:
                self.buffer = io.BytesIO(data)

            def __iter__(self) -> object:
                raise AssertionError("supervisor must not iterate unbounded text stdin")

        stdin = BufferOnlyStdin(oversized + valid)
        stdout = io.StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.assertEqual(main(), 0)

        lines = [line for line in stdout.getvalue().splitlines() if line]
        self.assertEqual(len(lines), 2)
        self.assertIn('"code":"message_too_large"', lines[0])
        self.assertIn('"kind":"pong"', lines[1])
        self.assertIn('"request_id":"ping-1"', lines[1])

    def test_supervisor_recovers_after_malformed_json(self) -> None:
        malformed = b"not-json\n"
        valid = (
            b'{"protocol_version":1,"request_id":"ping-2","operation":"ping","roots":[]}\n'
        )

        class BufferOnlyStdin:
            def __init__(self, data: bytes) -> None:
                self.buffer = io.BytesIO(data)

        stdin = BufferOnlyStdin(malformed + valid)
        stdout = io.StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.assertEqual(main(), 0)

        lines = [line for line in stdout.getvalue().splitlines() if line]
        self.assertEqual(len(lines), 2)
        self.assertIn('"code":"malformed_json"', lines[0])
        self.assertIn('"kind":"pong"', lines[1])
        self.assertIn('"request_id":"ping-2"', lines[1])

    def test_protocol_version_must_be_exact_integer_one(self) -> None:
        invalid = (
            '{"protocol_version":true,"request_id":"ping-4",'
            '"operation":"ping","roots":[]}'
        )

        with self.assertRaisesRegex(ProtocolError, "unsupported_protocol"):
            decode(invalid)

    def test_protocol_rejects_invalid_request_shape(self) -> None:
        with self.assertRaisesRegex(ProtocolError, "invalid_request"):
            decode(
                '{"protocol_version":1,"request_id":7,'
                '"operation":"ping","roots":[]}'
            )

        with self.assertRaisesRegex(ProtocolError, "invalid_roots"):
            decode(
                '{"protocol_version":1,"request_id":"scan-1",'
                '"operation":"scan","roots":"C:\\\\"}'
            )

        with self.assertRaisesRegex(ProtocolError, "unsupported_operation"):
            decode(
                '{"protocol_version":1,"request_id":"x",'
                '"operation":"execute","roots":[]}'
            )

    def test_protocol_write_rejects_oversized_output_without_writing(self) -> None:
        stream = io.StringIO()
        payload = {"kind": "discovery", "value": "x" * MAX_MESSAGE_BYTES}

        with self.assertRaisesRegex(ProtocolError, "message_too_large"):
            write(stream, payload)

        self.assertEqual(stream.getvalue(), "")

    def test_supervisor_recovers_after_invalid_utf8(self) -> None:
        invalid = b"\xff\n"
        valid = (
            b'{"protocol_version":1,"request_id":"ping-3","operation":"ping","roots":[]}\n'
        )

        class BufferOnlyStdin:
            def __init__(self, data: bytes) -> None:
                self.buffer = io.BytesIO(data)

        stdin = BufferOnlyStdin(invalid + valid)
        stdout = io.StringIO()

        with patch("sys.stdin", stdin), patch("sys.stdout", stdout):
            self.assertEqual(main(), 0)

        lines = [line for line in stdout.getvalue().splitlines() if line]
        self.assertEqual(len(lines), 2)
        self.assertIn('"code":"invalid_utf8"', lines[0])
        self.assertIn('"kind":"pong"', lines[1])
        self.assertIn('"request_id":"ping-3"', lines[1])

    def test_mcp_config_is_evidence_not_health(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "example-mcp.json"
            path.write_text('{"mcpServers": {"example": {}}}', encoding="utf-8")
            [result] = list(scan((directory,)))
            self.assertEqual(result["suggested_type"], "mcp")
            self.assertEqual(result["health_state"], "unknown")


if __name__ == "__main__":
    unittest.main()
