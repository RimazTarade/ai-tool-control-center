"""Fail safely when public-source candidates contain private or generated data."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

SKIP_DIRS = {".git", ".serena", "node_modules", "target", ".venv", "dist", "artifacts"}
BLOCKED_SUFFIXES = {".db", ".sqlite", ".log", ".bak", ".pfx", ".p12", ".key"}
TEXT_PATTERNS = {
    "absolute user path": re.compile(r"(?i)[a-z]:[\\/]users[\\/][^\\/\s]+"),
    "credential URL": re.compile(r"(?i)https?://[^/\s:@]+:[^/\s@]+@"),
    "private source": re.compile(r"(?i)(?:registry|index-url)\s*=.*(?:internal|corp|localhost|127\.0\.0\.1)"),
    "secret assignment": re.compile(
        r"(?i)(?:api[_-]?key|access[_-]?token|client[_-]?secret|password)\s*[:=]\s*['\"][^'\"]{8,}"
    ),
    "private key": re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
}


def scan(root: Path) -> list[tuple[Path, str, int | None]]:
    findings: list[tuple[Path, str, int | None]] = []
    for path in root.rglob("*"):
        if any(part in SKIP_DIRS for part in path.relative_to(root).parts):
            continue
        if not path.is_file():
            continue
        relative = path.relative_to(root)
        if path.suffix.lower() in BLOCKED_SUFFIXES:
            findings.append((relative, "generated or sensitive file", None))
            continue
        if path.stat().st_size > 2_000_000:
            continue
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            continue
        for number, line in enumerate(lines, 1):
            for category, pattern in TEXT_PATTERNS.items():
                if pattern.search(line):
                    findings.append((relative, category, number))
    return findings


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", nargs="?", default=".", type=Path)
    args = parser.parse_args()
    findings = scan(args.root.resolve())
    for path, category, line in findings:
        location = f"{path}:{line}" if line else str(path)
        print(f"{location}: {category}")
    print(f"repository policy: {len(findings)} finding(s)")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
