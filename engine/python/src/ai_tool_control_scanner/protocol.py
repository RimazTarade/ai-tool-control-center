from __future__ import annotations

import json
import re
from dataclasses import dataclass
from typing import Any, BinaryIO, TextIO

MAX_MESSAGE_BYTES = 1_048_576
SECRET = re.compile(
    r"(?i)(authorization|api[_-]?key|token|password)\s*[:=]\s*([^,\s}\"]+)"
)


class ProtocolError(ValueError):
    pass


@dataclass(frozen=True)
class Request:
    request_id: str
    operation: str
    roots: tuple[str, ...]


def read_bounded_line(stream: BinaryIO) -> str | None:
    chunk = stream.readline(MAX_MESSAGE_BYTES + 1)
    if not chunk:
        return None

    if len(chunk) > MAX_MESSAGE_BYTES:
        while chunk and not chunk.endswith(b"\n"):
            chunk = stream.readline(8192)
        raise ProtocolError("message_too_large")

    try:
        return chunk.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ProtocolError("invalid_utf8") from error


def decode(line: str) -> Request:
    if len(line.encode("utf-8")) > MAX_MESSAGE_BYTES:
        raise ProtocolError("message_too_large")
    try:
        value: Any = json.loads(line)
    except json.JSONDecodeError as error:
        raise ProtocolError("malformed_json") from error
    if not isinstance(value, dict):
        raise ProtocolError("unsupported_protocol")

    protocol_version = value.get("protocol_version")
    if type(protocol_version) is not int or protocol_version != 1:
        raise ProtocolError("unsupported_protocol")
    request_id = value.get("request_id")
    operation = value.get("operation")
    roots = value.get("roots", [])
    if not isinstance(request_id, str) or not isinstance(operation, str):
        raise ProtocolError("invalid_request")
    if not isinstance(roots, list) or not all(isinstance(root, str) for root in roots):
        raise ProtocolError("invalid_roots")
    if operation not in {"scan", "cancel", "ping"}:
        raise ProtocolError("unsupported_operation")
    return Request(request_id, operation, tuple(roots))


def write(stream: TextIO, payload: dict[str, Any]) -> None:
    encoded = json.dumps({"protocol_version": 1, **payload}, separators=(",", ":"))
    if len(encoded.encode("utf-8")) > MAX_MESSAGE_BYTES:
        raise ProtocolError("message_too_large")
    stream.write(encoded + "\n")
    stream.flush()


def redact(message: str) -> str:
    return SECRET.sub(lambda match: f"{match.group(1)}=[REDACTED]", message)
