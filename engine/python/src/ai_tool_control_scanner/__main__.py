from __future__ import annotations

import sys

from .protocol import ProtocolError, decode, read_bounded_line, redact, write
from .scanners import scan


def main() -> int:
    cancelled: set[str] = set()
    while True:
        try:
            line = read_bounded_line(sys.stdin.buffer)
            if line is None:
                break
            request = decode(line)
            if request.operation == "ping":
                write(sys.stdout, {"request_id": request.request_id, "kind": "pong"})
            elif request.operation == "cancel":
                cancelled.add(request.request_id)
                write(sys.stdout, {"request_id": request.request_id, "kind": "cancelled"})
            else:
                count = 0
                for item in scan(request.roots):
                    if request.request_id in cancelled:
                        break
                    write(sys.stdout, {"request_id": request.request_id, "kind": "discovery", "discovery": item})
                    count += 1
                write(sys.stdout, {"request_id": request.request_id, "kind": "completed", "count": count})
        except ProtocolError as error:
            write(sys.stdout, {"kind": "error", "code": str(error)})
        except Exception as error:  # scanner faults must not terminate the protocol supervisor
            write(sys.stdout, {"kind": "error", "code": "scanner_failed", "message": redact(str(error))})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
