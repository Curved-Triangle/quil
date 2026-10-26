#!/usr/bin/env python3
"""Explicitly fetch and verify the pinned macOS arm64 ONNX Runtime archive.

This is a build-time action, never invoked by Quil's dictation runtime. It uses
only Python's standard library and installs one static library under .tools/.
"""

from __future__ import annotations

import hashlib
import lzma
import os
from pathlib import Path
import shutil
import tarfile
import urllib.request

URL = "https://cdn.pyke.io/0/pyke:ort-rs/ms@1.31.0/aarch64-apple-darwin+coreml.tar.lzma2"
ARCHIVE_BYTES = 9_687_276
ARCHIVE_SHA256 = "4033efca0e6c1606bd2ec3870d4cb19d0dba0ad0160dbfb616e66cae969ed68c"
TAR_BYTES = 83_547_648
LIB_BYTES = 83_546_072
LIB_SHA256 = "8a9d7b9ce57feaa0456df9b5c158c4c0ff948925edb58f523126a3bf27136641"

ROOT = Path(__file__).resolve().parent.parent
DOWNLOAD = ROOT / ".tools" / "downloads" / "onnxruntime.tar.lzma2"
TAR = ROOT / ".tools" / "downloads" / "onnxruntime.tar"
LIB = ROOT / ".tools" / "onnxruntime" / "lib" / "libonnxruntime.a"


def digest_file(path: Path) -> tuple[int, str]:
    hasher = hashlib.sha256()
    size = 0
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            size += len(chunk)
            hasher.update(chunk)
    return size, hasher.hexdigest()


def require_digest(path: Path, expected_bytes: int, expected_sha256: str) -> None:
    size, actual = digest_file(path)
    if size != expected_bytes or actual != expected_sha256:
        raise RuntimeError(
            f"integrity check failed for {path}: {size} bytes / {actual}; "
            f"expected {expected_bytes} bytes / {expected_sha256}"
        )


def download() -> None:
    if DOWNLOAD.exists():
        require_digest(DOWNLOAD, ARCHIVE_BYTES, ARCHIVE_SHA256)
        return
    DOWNLOAD.parent.mkdir(parents=True, exist_ok=True)
    temporary = DOWNLOAD.with_suffix(".download")
    hasher = hashlib.sha256()
    size = 0
    try:
        request = urllib.request.Request(URL, headers={"User-Agent": "quil-build/0.1"})
        with urllib.request.urlopen(request, timeout=120) as response, temporary.open("xb") as output:
            if not response.geturl().startswith("https://"):
                raise RuntimeError("ONNX Runtime redirect left HTTPS")
            for chunk in iter(lambda: response.read(1024 * 1024), b""):
                size += len(chunk)
                if size > ARCHIVE_BYTES:
                    raise RuntimeError("ONNX Runtime download exceeded the pinned size")
                hasher.update(chunk)
                output.write(chunk)
        if size != ARCHIVE_BYTES or hasher.hexdigest() != ARCHIVE_SHA256:
            raise RuntimeError("ONNX Runtime archive did not match the pinned digest")
        os.replace(temporary, DOWNLOAD)
    finally:
        temporary.unlink(missing_ok=True)


def extract() -> None:
    require_digest(DOWNLOAD, ARCHIVE_BYTES, ARCHIVE_SHA256)
    decompressor = lzma.LZMADecompressor(
        format=lzma.FORMAT_RAW,
        filters=[{"id": lzma.FILTER_LZMA2, "preset": 9}],
    )
    with DOWNLOAD.open("rb") as source, TAR.open("wb") as output:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            output.write(decompressor.decompress(chunk))
    if not decompressor.eof or TAR.stat().st_size != TAR_BYTES:
        raise RuntimeError("unexpected ONNX Runtime tar stream")

    with tarfile.open(TAR, mode="r:") as archive:
        members = archive.getmembers()
        if len(members) != 1 or members[0].name != "libonnxruntime.a" or not members[0].isfile():
            raise RuntimeError("unexpected files in ONNX Runtime archive")
        source = archive.extractfile(members[0])
        if source is None:
            raise RuntimeError("libonnxruntime.a is unreadable")
        LIB.parent.mkdir(parents=True, exist_ok=True)
        temporary = LIB.with_suffix(".download")
        with source, temporary.open("wb") as output:
            shutil.copyfileobj(source, output, length=1024 * 1024)
    require_digest(temporary, LIB_BYTES, LIB_SHA256)
    os.replace(temporary, LIB)


def main() -> None:
    download()
    extract()
    require_digest(LIB, LIB_BYTES, LIB_SHA256)
    print(f"Verified ONNX Runtime 1.31.0: {LIB}")


if __name__ == "__main__":
    main()
