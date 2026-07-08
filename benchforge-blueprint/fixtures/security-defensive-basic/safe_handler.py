from pathlib import Path


def read_note(root: Path, name: str) -> str:
    candidate = (root / name).resolve()
    root = root.resolve()
    if root not in candidate.parents and candidate != root:
        raise ValueError("path escapes root")
    return candidate.read_text(encoding="utf-8")
