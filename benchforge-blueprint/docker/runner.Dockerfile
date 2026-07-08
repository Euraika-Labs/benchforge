FROM python:3.12-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    git curl ca-certificates nodejs npm \
    && rm -rf /var/lib/apt/lists/*

RUN python -m pip install --no-cache-dir pytest

WORKDIR /workspace

# Keep this image boring. Benchmark packs can layer their own images later.
CMD ["bash"]
