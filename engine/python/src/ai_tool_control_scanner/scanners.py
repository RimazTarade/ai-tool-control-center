from __future__ import annotations

import hashlib
import json
import tomllib
from collections.abc import Iterator
from pathlib import Path
from typing import Any

NAMES = {"claude", "codex", "mcp", "docker", "ollama"}
SUFFIXES = {".json", ".yaml", ".yml", ".toml"}


def _path_product(path: Path) -> str | None:
    name = path.name.casefold()
    parts = {part.casefold() for part in path.parts}

    for product in NAMES:
        if product in name or f".{product}" in parts:
            return product
    return None


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
            if _path_product(path) is None:
                continue
            yield discovery(path)


def discovery(path: Path) -> dict[str, Any]:
    product = _path_product(path)
    kind = product or "unknown"
    evidence = (
        f"configuration path matches {product}"
        if product is not None
        else "matching configuration filename"
    )
    try:
        if path.suffix.casefold() == ".json":
            data: Any = json.loads(path.read_text(encoding="utf-8"))
        elif path.suffix.casefold() == ".toml":
            data = tomllib.loads(path.read_text(encoding="utf-8"))
        else:
            data = None

        if isinstance(data, dict) and (
            isinstance(data.get("mcpServers"), dict)
            or isinstance(data.get("mcp_servers"), dict)
        ):
            if product == "mcp":
                kind = "mcp"
                evidence = "configuration contains an MCP server mapping"
            else:
                evidence = f"{product} configuration contains an MCP server mapping"
    except (OSError, UnicodeError, json.JSONDecodeError, tomllib.TOMLDecodeError):
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
