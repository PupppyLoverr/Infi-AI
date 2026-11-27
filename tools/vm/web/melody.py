#!/usr/bin/env python3
"""Fallback song for the web media test: a gentle 3/4 melody (in the manner
of a slow waltz), synthesised so the test never depends on a download."""
import math, struct, sys, wave
rate = 44100
notes = [(67, 2), (74, 1), (71, 1), (69, 1), (71, 1), (74, 3), (66, 3),
         (67, 2), (69, 1), (71, 3), (69, 3), (67, 2), (64, 1), (66, 3)] * 4
def tone(midi, beats, bpm=72):
    f = 440 * 2 ** ((midi - 69) / 12); n = int(rate * beats * 60 / bpm)
    return [0.28 * math.sin(2 * math.pi * f * i / rate) * min(1, i / 800) * math.exp(-1.6 * i / n) for i in range(n)]
samples = [s for m, b in notes for s in tone(m, b)]
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes(b"".join(struct.pack("<h", int(max(-1, min(1, s)) * 32000)) for s in samples))
