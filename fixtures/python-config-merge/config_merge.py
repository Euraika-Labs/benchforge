def merge_config(base: dict, override: dict) -> dict:
    """Merge override values into base configuration."""
    merged = dict(base)
    merged.update(override)
    return merged
