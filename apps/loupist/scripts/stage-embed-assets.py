#!/usr/bin/env python3
"""Stage the pinned local embedding model without a separate runtime.

The tokenizer and config are the upstream files at the pinned revision. The
embedding table is the upstream float32 table stored as int8 with one float32
scale per row (largest absolute value / 127), as the `weights` tensor that
model2vec applies per token. Each quotient is rounded to float32 the way a
float32 division would round it, so the derived file is byte-for-byte the
pinned one without numpy.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import struct
import sys
import tempfile
import time
import urllib.error
import urllib.request

from check_embed_assets import MODEL_FILES, digest, load_pin, verify

APP = Path(__file__).resolve().parent.parent
SOURCE = APP.parent.parent / "src/knowledge/embed_assets"
DESTINATION = APP / "src-tauri/resources/embed_assets"
DOWNLOAD_BUDGET = 512 * 1024 * 1024


def download(url: str, destination: Path) -> None:
    request = urllib.request.Request(url, headers={"User-Agent": "loupist-staging"})
    for attempt in range(3):
        try:
            with (
                urllib.request.urlopen(request, timeout=120) as response,
                destination.open("wb") as output,
            ):
                size = 0
                for block in iter(lambda: response.read(1024 * 1024), b""):
                    size += len(block)
                    if size > DOWNLOAD_BUDGET:
                        raise SystemExit(
                            f"Embedding download exceeds its budget: {url}"
                        )
                    output.write(block)
            return
        except urllib.error.HTTPError as error:
            if error.code not in (502, 503, 504) or attempt == 2:
                raise
            error.close()
            time.sleep(2**attempt)


def fetch_upstream(pin: dict, upstream: Path | None, directory: Path) -> Path:
    """The pinned upstream files, from `upstream` or downloaded into `directory`."""
    if upstream is None:
        for name in sorted(MODEL_FILES):
            url = f"https://huggingface.co/{pin['model']}/resolve/{pin['revision']}/{name}"
            download(url, directory / name)
        upstream = directory
    for name in sorted(MODEL_FILES):
        path = upstream / name
        if path.is_symlink() or not path.is_file():
            raise SystemExit(f"Missing upstream embedding file: {path}")
        if digest(path) != pin["upstream"][name]:
            raise SystemExit(f"Upstream embedding file does not match its pin: {name}")
    return upstream


def derive_int8(upstream: Path, output: Path) -> None:
    if sys.byteorder != "little":
        raise SystemExit("Deriving the embedding table needs a little-endian host")
    data = upstream.read_bytes()
    size = struct.unpack("<Q", data[:8])[0]
    spec = json.loads(data[8 : 8 + size])["embeddings"]
    if spec["dtype"] != "F32" or len(spec["shape"]) != 2:
        raise SystemExit("Upstream embedding table is not a 2-D float32 tensor")
    rows, cols = spec["shape"]
    start, end = (8 + size + offset for offset in spec["data_offsets"])
    if end - start != rows * cols * 4:
        raise SystemExit("Upstream embedding table has an inconsistent size")
    table = memoryview(data)[start:end].cast("f")
    row_format = struct.Struct(f"<{cols}f")
    quantized = bytearray(rows * cols)
    scales = bytearray(4 * rows)
    for row in range(rows):
        values = table[row * cols : (row + 1) * cols]
        scale = struct.unpack("<f", struct.pack("<f", max(map(abs, values)) / 127.0))[0]
        struct.pack_into("<f", scales, 4 * row, scale)
        if scale == 0.0:
            continue
        levels = row_format.unpack(
            row_format.pack(*[value / scale for value in values])
        )
        quantized[row * cols : (row + 1) * cols] = bytes(
            max(-127, min(127, round(level))) & 0xFF for level in levels
        )
    header = {
        "embeddings": {
            "dtype": "I8",
            "shape": [rows, cols],
            "data_offsets": [0, len(quantized)],
        },
        "weights": {
            "dtype": "F32",
            "shape": [rows],
            "data_offsets": [len(quantized), len(quantized) + len(scales)],
        },
    }
    encoded = json.dumps(header, separators=(",", ":")).encode()
    encoded += b" " * (-len(encoded) % 8)
    with output.open("wb") as stream:
        stream.write(struct.pack("<Q", len(encoded)))
        stream.write(encoded)
        stream.write(quantized)
        stream.write(scales)


def stage(source: Path, destination: Path, upstream: Path | None) -> None:
    pin = load_pin(source)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="embed-stage-", dir=destination.parent
    ) as temporary:
        downloads = Path(temporary) / "upstream"
        downloads.mkdir()
        upstream = fetch_upstream(pin, upstream, downloads)
        staged = Path(temporary) / "embed_assets"
        staged.mkdir()
        for name in ("config.json", "tokenizer.json"):
            shutil.copyfile(upstream / name, staged / name)
        derive_int8(upstream / "model.safetensors", staged / "model.safetensors")
        shutil.copyfile(source / "NOTICE", staged / "NOTICE")
        try:
            verify(source, staged)
        except ValueError as error:
            raise SystemExit(str(error)) from error
        if destination.exists():
            shutil.rmtree(destination)
        staged.rename(destination)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=SOURCE)
    parser.add_argument("--destination", type=Path, default=DESTINATION)
    parser.add_argument(
        "--upstream",
        type=Path,
        help="directory holding the pinned upstream files instead of downloading them",
    )
    parser.add_argument(
        "--ensure", action="store_true", help="keep assets that already match the pins"
    )
    parser.add_argument(
        "--verify", action="store_true", help="check the staged assets only"
    )
    args = parser.parse_args()
    if args.verify or args.ensure:
        try:
            verify(args.source, args.destination)
            print("Staged local embedding assets match their pins.")
            return
        except (OSError, ValueError) as error:
            if args.verify:
                raise SystemExit(
                    f"{error}; run python3 apps/loupist/scripts/stage-embed-assets.py"
                ) from error
    stage(args.source, args.destination, args.upstream)
    print("Staged local embedding assets.")


if __name__ == "__main__":
    main()
