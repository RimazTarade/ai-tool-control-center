from __future__ import annotations

import hashlib
import json
from collections.abc import Iterator
from pathlib import Path
from typing import Any

NAMES = {"claude", "codex", "mcp", "docker", "ollama"}
SUFFIXES = {".json", ".yaml", ".yml", ".toml"}


def scan(roots: tuple[str, ...]) -> Iterator[dict[str, Any]]:
    for raw_root in roots:
        root = Path(raw_root)
        if not root.is_absolute() or not root.is_dir():
            continue
        for path in root.rglob("*"):
            if any(part.lower() in {".git", "node_modules", ".venv"} for part in path.parts):
                continue
            if not path.is_file() or path.suffix.lower() not in SUFFIXES:
                continue
            if not any(name in path.name.lower() for name in NAMES):
                continue
            yield discovery(path)


def discovery(path: Path) -> dict[str, Any]:
    kind = "unknown"
    evidence = "matching configuration filename"
    try:
        data = json.loads(path.read_text(encoding="utf-8")) if path.suffix.lower() == ".json" else None
        if isinstance(data, dict) and ("mcpServers" in data or "mcp_servers" in data):
            kind = "mcp"
            evidence = "configuration contains an MCP server mapping"
    except (OSError, UnicodeError, json.JSONDecodeError):
        pass
    fingerprint = hashlib.sha256(str(path).casefold().encode()).hexdigest()
    return {
        "fingerprint": fingerprint,
        "suggested_name": path.stem,
        "suggested_type": kind,
        "confidence": "medium" if kind != "unknown" else "low",
        "evidence": [{"kind": "path", "summary": str(path)}, {"kind": "reason", "summary": evidence}],
        "health_state": "unknown",
    }
