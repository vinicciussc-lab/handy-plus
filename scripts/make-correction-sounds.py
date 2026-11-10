"""Generates the two soft sounds of the text correction.

Run: python -I scripts/make-correction-sounds.py
Writes src-tauri/resources/correction_start.wav and correction_ready.wav
(44.1 kHz, mono, 16 bit). Sine tones with raised-cosine attack and release,
so there are no clicks and nothing is harsh.
"""

import math
import struct
import wave
from pathlib import Path

RATE = 44100
OUT = Path(__file__).resolve().parent.parent / "src-tauri" / "resources"


def tone(freq, ms, attack_ms, release_ms):
    n = int(RATE * ms / 1000)
    attack = int(RATE * attack_ms / 1000)
    release = int(RATE * release_ms / 1000)
    out = []
    for i in range(n):
        env = 1.0
        if i < attack:
            env = 0.5 - 0.5 * math.cos(math.pi * i / attack)
        if i >= n - release:
            env = min(env, 0.5 - 0.5 * math.cos(math.pi * (n - 1 - i) / release))
        out.append(env * math.sin(2 * math.pi * freq * i / RATE))
    return out


def write(name, samples, peak_dbfs):
    peak = max(abs(x) for x in samples)
    gain = (10 ** (peak_dbfs / 20)) / peak
    data = b"".join(struct.pack("<h", round(x * gain * 32767)) for x in samples)
    with wave.open(str(OUT / name), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(data)
    print(f"{name}: {len(samples) * 1000 / RATE:.0f} ms, peak {peak_dbfs} dBFS")


# Two short rising tones, 660 Hz then 880 Hz.
start = tone(660, 90, 15, 15) + tone(880, 90, 15, 15)
write("correction_start.wav", start, -18)

# One longer tone with a long fade out.
write("correction_ready.wav", tone(880, 140, 15, 100), -20)
