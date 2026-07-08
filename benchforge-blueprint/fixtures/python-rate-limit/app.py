from dataclasses import dataclass

USERS = {"alice": "correct-horse-battery-staple"}

@dataclass
class Response:
    status_code: int
    body: dict


def login(username: str, password: str, ip_address: str) -> Response:
    """Simple login handler used by the benchmark fixture."""
    if USERS.get(username) == password:
        return Response(200, {"ok": True})
    return Response(401, {"ok": False, "error": "invalid credentials"})
