# python wavcmp.py <desktop.wav> <page.wav> [--per-second]
# The desktop's mix (FMOD 1.08 WAV writer) against the page's (FMOD 2.03 HTML5, caught at the
# master group). Both are 48 kHz 16-bit stereo; their setup mixes a different number of blocks,
# so the page's is shifted by whole 800-sample blocks to where the loudness envelopes agree best.
# Prints loudness (RMS dBFS) and the spectrum (octave bands) per second, and the differences.
import sys
import wave

import numpy as np

BLOCK = 800
BANDS = [(31, 63), (63, 125), (125, 250), (250, 500), (500, 1000), (1000, 2000), (2000, 4000), (4000, 8000), (8000, 16000), (16000, 24000)]


def read(path):
    with wave.open(path) as w:
        assert w.getframerate() == 48000 and w.getsampwidth() == 2, path
        x = np.frombuffer(w.readframes(w.getnframes()), dtype='<i2').astype(np.float64) / 32768
        return x.reshape(-1, w.getnchannels()).mean(axis=1)


def envelope(x):
    n = len(x) // BLOCK
    return 10 * np.log10(np.mean(x[: n * BLOCK].reshape(n, BLOCK) ** 2, axis=1) + 1e-12)


def db(x):
    return 10 * np.log10(np.mean(x ** 2) + 1e-12)


def bands(x):
    spec = np.abs(np.fft.rfft(x * np.hanning(len(x)))) ** 2
    freq = np.fft.rfftfreq(len(x), 1 / 48000)
    return [10 * np.log10(spec[(freq >= lo) & (freq < hi)].sum() + 1e-12) for lo, hi in BANDS]


a, b = read(sys.argv[1]), read(sys.argv[2])
ea, eb = envelope(a), envelope(b)
best, shift = None, 0
for s in range(-300, 301):
    lo, hi = max(0, s), min(len(ea), len(eb) + s)
    if hi - lo < 60:
        continue
    d = np.mean(np.abs(ea[lo:hi] - eb[lo - s:hi - s]))
    if best is None or d < best:
        best, shift = d, s
# a = desktop block i  <->  page block i - shift
start_a, start_b = max(0, shift), max(0, -shift)
n = min(len(a) // BLOCK - start_a, len(b) // BLOCK - start_b)
a, b = a[start_a * BLOCK:(start_a + n) * BLOCK], b[start_b * BLOCK:(start_b + n) * BLOCK]
seconds = n * BLOCK // 48000
rows = []
for s in range(seconds):
    sa, sb = a[s * 48000:(s + 1) * 48000], b[s * 48000:(s + 1) * 48000]
    ba, bb = bands(sa), bands(sb)
    rows.append((s, db(sa), db(sb), [y - x for x, y in zip(ba, bb)]))
la, lb = db(a), db(b)
spread = [abs(r[2] - r[1]) for r in rows]
band_diff = np.array([r[3] for r in rows]) if rows else np.zeros((0, len(BANDS)))
print(f'shift {shift} blocks, {seconds} s compared; whole: desktop {la:.1f} dBFS, page {lb:.1f} dBFS ({lb - la:+.1f} dB); '
      f'per second |diff| mean {np.mean(spread):.1f} dB, max {np.max(spread):.1f} dB')
print('bands (Hz):      ' + ' '.join(f'{lo:>6}' for lo, _ in BANDS))
print('mean diff (dB):  ' + ' '.join(f'{v:+6.1f}' for v in band_diff.mean(axis=0)))
if '--per-second' in sys.argv:
    print('second | desktop dBFS | page dBFS | diff | band diffs (dB, page - desktop)')
    for s, x, y, d in rows:
        print(f'{s:>6} | {x:12.1f} | {y:9.1f} | {y - x:+4.1f} | ' + ' '.join(f'{v:+5.1f}' for v in d))
