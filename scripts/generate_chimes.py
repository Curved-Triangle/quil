#!/usr/bin/env python3
"""Generate Quil's original, MIT-licensed notification sounds (no samples)."""

from __future__ import annotations

import math
from pathlib import Path
import struct
import wave

ROOT = Path(__file__).resolve().parent.parent
RATE = 16_000


def write_chime(path: Path, notes: list[tuple[float, float]], peak: float = 0.08) -> None:
    duration = sum(duration for _, duration in notes)
    samples = []
    for index in range(int(RATE * duration)):
        time = index / RATE
        elapsed = 0.0
        value = 0.0
        for frequency, note_duration in notes:
            if elapsed <= time < elapsed + note_duration:
                local = (time - elapsed) / note_duration
                envelope = min(1.0, local * 40) * min(1.0, (1.0 - local) * 12)
                value += math.sin(2 * math.pi * frequency * (time - elapsed)) * envelope
            elapsed += note_duration
        samples.append(int(max(-1.0, min(1.0, value * peak)) * 32767))
    path.parent.mkdir(parents=True, exist_ok=True)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(RATE)
        output.writeframes(struct.pack(f"<{len(samples)}h", *samples))


def main() -> None:
    write_chime(ROOT / "assets" / "chime-start.wav", [(880.0, 0.07), (1320.0, 0.13)])
    write_chime(ROOT / "assets" / "chime-stop.wav", [(660.0, 0.06), (520.0, 0.12)])
    print("Generated original chimes under assets/")


if __name__ == "__main__":
    main()
