#!/usr/bin/env python3
"""Prepare a small, attributed Mini LibriSpeech benchmark and CI fixture.

The archive is fetched only when this script is run explicitly. No Quil capture
can reach this corpus path. Python and macOS afconvert are the only requirements.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

URL = "https://openslr.trmal.net/resources/31/dev-clean-2.tar.gz"
BYTES = 126_046_265
MD5 = "6d7ab67ac6a1d2c993d050e16d61080d"
SHA256 = "176ec501490eced2d6c1f89f4f0ddc7dfe799e649e5322f8ba49fe3ff50c8012"
CLIPS_PER_SPEAKER = 2
SPEAKERS = 4

ROOT = Path(__file__).resolve().parent.parent
WORK = ROOT / "benchmark-data" / "corpus"
ARCHIVE = WORK / "dev-clean-2.tar.gz"
MANIFEST = WORK / "benchmark-corpus.json"
FIXTURES = ROOT / "tests" / "fixtures"


def verify_archive() -> None:
    md5 = hashlib.md5(usedforsecurity=False)
    sha256 = hashlib.sha256()
    size = 0
    with ARCHIVE.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            size += len(chunk)
            md5.update(chunk)
            sha256.update(chunk)
    if size != BYTES or md5.hexdigest() != MD5 or sha256.hexdigest() != SHA256:
        raise RuntimeError("Mini LibriSpeech archive failed size/MD5/SHA-256 verification")


def download_archive() -> None:
    if ARCHIVE.exists():
        verify_archive()
        return
    WORK.mkdir(parents=True, exist_ok=True)
    temporary = ARCHIVE.with_suffix(".download")
    md5 = hashlib.md5(usedforsecurity=False)
    sha256 = hashlib.sha256()
    size = 0
    try:
        request = urllib.request.Request(URL, headers={"User-Agent": "quil-benchmark/0.1"})
        with urllib.request.urlopen(request, timeout=120) as response, temporary.open("xb") as output:
            if not response.geturl().startswith("https://"):
                raise RuntimeError("corpus redirect left HTTPS")
            for chunk in iter(lambda: response.read(1024 * 1024), b""):
                size += len(chunk)
                if size > BYTES:
                    raise RuntimeError("corpus download exceeded the pinned size")
                md5.update(chunk)
                sha256.update(chunk)
                output.write(chunk)
        if size != BYTES or md5.hexdigest() != MD5 or sha256.hexdigest() != SHA256:
            raise RuntimeError("downloaded corpus failed integrity verification")
        os.replace(temporary, ARCHIVE)
    finally:
        temporary.unlink(missing_ok=True)


def selected_utterances(archive: tarfile.TarFile) -> list[tuple[str, tarfile.TarInfo, tarfile.TarInfo]]:
    by_speaker: dict[str, list[tuple[str, tarfile.TarInfo, tarfile.TarInfo]]] = {}
    for member in archive.getmembers():
        if not member.isfile():
            continue
        path = Path(member.name)
        if path.suffix == ".flac":
            # LibriSpeech names transcripts <speaker>-<chapter>.trans.txt.
            transcript = path.parent / f"{path.parent.parent.name}-{path.parent.name}.trans.txt"
            trans_info = archive.getmember(str(transcript))
            utterance_id = path.stem
            by_speaker.setdefault(path.parts[2], []).append((utterance_id, member, trans_info))

    selected = []
    for speaker in sorted(by_speaker)[:SPEAKERS]:
        clips = sorted(by_speaker[speaker], key=lambda item: item[0])[:CLIPS_PER_SPEAKER]
        selected.extend(clips)
    if not selected:
        raise RuntimeError("no utterances selected from corpus")
    return selected


def prepare() -> None:
    download_archive()
    verify_archive()
    (WORK / "wav").mkdir(parents=True, exist_ok=True)
    FIXTURES.mkdir(parents=True, exist_ok=True)
    entries = []
    with tempfile.TemporaryDirectory(prefix="extract-", dir=WORK) as temporary:
        temp = Path(temporary)
        with tarfile.open(ARCHIVE, mode="r:gz") as archive:
            selected = selected_utterances(archive)
            for index, (utterance_id, audio_info, trans_info) in enumerate(selected):
                flac = temp / f"{utterance_id}.flac"
                with archive.extractfile(audio_info) as source, flac.open("wb") as output:
                    if source is None:
                        raise RuntimeError(f"cannot read {audio_info.name}")
                    shutil.copyfileobj(source, output)
                with archive.extractfile(trans_info) as source:
                    if source is None:
                        raise RuntimeError(f"cannot read {trans_info.name}")
                    lines = source.read().decode("utf-8").splitlines()
                reference = next(
                    (line[len(utterance_id) :].strip() for line in lines if line.startswith(utterance_id + " ")),
                    None,
                )
                if reference is None:
                    raise RuntimeError(f"reference transcript missing for {utterance_id}")
                wav = WORK / "wav" / f"{utterance_id}.wav"
                subprocess.run(
                    ["afconvert", "-f", "WAVE", "-d", "LEI16@16000", str(flac), str(wav)],
                    check=True,
                    stdout=subprocess.DEVNULL,
                )
                entries.append({"audio": f"wav/{utterance_id}.wav", "reference": reference})
                if index == 0:
                    fixture_wav = FIXTURES / "librispeech-sample.wav"
                    shutil.copyfile(wav, fixture_wav)
                    fixture_manifest = {
                        "name": "mini-librispeech-ci-smoke",
                        "files": [{"audio": fixture_wav.name, "reference": reference}],
                    }
                    (FIXTURES / "benchmark-corpus.json").write_text(
                        json.dumps(fixture_manifest, indent=2) + "\n", encoding="utf-8"
                    )
                    (FIXTURES / "NOTICE.md").write_text(
                        "# Regression fixture attribution\n\n"
                        f"`librispeech-sample.wav` is utterance `{utterance_id}` from **Mini LibriSpeech, OpenSLR SLR31 (`dev-clean-2`)**.\n\n"
                        "- Source: <https://www.openslr.org/31>\n"
                        "- Attribution: Mini LibriSpeech, OpenSLR SLR31\n"
                        "- License: Creative Commons Attribution 4.0 International; see `LICENSE-CC-BY-4.0.txt` and <https://creativecommons.org/licenses/by/4.0/>\n"
                        "- Change: the source FLAC was converted to 16 kHz signed 16-bit PCM WAV; the audio was not edited.\n"
                        f"- Reference transcript: {reference}\n\n"
                        "This is external corpus speech, not a Quil microphone recording.\n",
                        encoding="utf-8",
                    )

    manifest = {"name": "mini-librispeech-dev-clean-2-eight-clips", "files": entries}
    MANIFEST.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"Prepared {len(entries)} clips: {MANIFEST}")


if __name__ == "__main__":
    prepare()
