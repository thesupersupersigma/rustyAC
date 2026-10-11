# SPDX-License-Identifier: MIT OR Apache-2.0
# Turns the raw clips of audio.sh into the site's files: one common gain, loops made seamless,
# one-shots trimmed, 48 kHz 16-bit stereo WAV, and manifest.json.
# usage: python post.py <raw folder> <out folder> <rustyAC version>
import json, os, sys, wave
import numpy as np

SR = 48000
raw_dir, out_dir, version = sys.argv[1], sys.argv[2], sys.argv[3]
os.makedirs(out_dir, exist_ok=True)


def load(name):
    w = wave.open(os.path.join(raw_dir, name + '.wav'), 'rb')
    assert (w.getnchannels(), w.getsampwidth(), w.getframerate()) == (2, 2, SR), name
    return np.frombuffer(w.readframes(w.getnframes()), '<i2').astype(np.float64).reshape(-1, 2) / 32768.0


def save(name, a):
    pcm = np.clip(np.round(a * 32768.0), -32768, 32767).astype('<i2')
    w = wave.open(os.path.join(out_dir, name + '.wav'), 'wb')
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(SR)
    w.writeframes(pcm.tobytes())
    w.close()
    return pcm


def db(x):
    return float(20 * np.log10(max(x, 1e-9)))


def make_loop(a, start=3 * SR, length=8 * SR, fade=int(0.5 * SR), search=int(0.25 * SR)):
    """A loop of about `length` samples cut from `a` after `start`: the loop's end is searched
    (within `search` samples) for the place where what follows it is most like what precedes
    the start, then the last `fade` samples are crossfaded into the samples before the start,
    so that the last sample runs into the first as the recording itself ran on."""
    mono = a.mean(axis=1)
    head = mono[start - fade:start]
    best, best_score = None, -2.0
    for end in range(start + length - search, start + length + search, 8):
        tail = mono[end - fade:end]
        score = float(np.dot(head, tail) / (np.linalg.norm(head) * np.linalg.norm(tail) + 1e-12))
        if score > best_score:
            best, best_score = end, score
    end = best
    loop = a[start:end].copy()
    # equal-power when the two are unlike, linear when they are alike
    t = np.linspace(0.0, 1.0, fade, endpoint=False)[:, None]
    c = max(best_score, 0.0)
    norm = np.sqrt(t * t + (1 - t) * (1 - t) + 2 * c * t * (1 - t))
    loop[-fade:] = (a[end - fade:end] * (1 - t) + a[start - fade:start] * t) / norm
    return loop, best_score


def seam(pcm):
    """The jump across the loop point against the jumps inside the file (16-bit units)."""
    x = pcm.astype(np.float64)
    inside = np.abs(np.diff(x, axis=0))
    across = np.abs(x[0] - x[-1])
    return float(across.max()), float(np.percentile(inside, 99.9)), float(inside.max())


def pitch(a):
    """The spectrum's centre of gravity and its strongest line below 2 kHz, Hz."""
    mono = a.mean(axis=1)[: 4 * SR]
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono))))
    freq = np.fft.rfftfreq(len(mono), 1 / SR)
    low = freq < 2000
    return float((spec * freq).sum() / spec.sum()), float(freq[low][np.argmax(spec[low])])


def trim_oneshot(a, pre=0.01, floor_db=-66.0, tail=0.05):
    m = np.abs(a).max(axis=1)
    level = 10 ** (floor_db / 20) * m.max()
    idx = np.nonzero(m > level)[0]
    first = max(int(idx[0] - pre * SR), 0)
    last = min(int(idx[-1] + tail * SR), len(a))
    out = a[first:last].copy()
    n = int(0.005 * SR)
    out[-n:] *= np.linspace(1, 0, n)[:, None]
    return out


RPM = {'idle': 4000, '4000': 4000, '7000': 7000, '10000': 10000, '13000': 13000, '16000': 16000}
ENGINE = 'event:/cars/ks_ferrari_f2004/engine_ext'
SKID = 'event:/cars/ks_ferrari_f2004/skid_ext'
WHEEL = 'event:/cars/ks_ferrari_f2004/wheel'
clips = []
for key, rpm in RPM.items():
    clips.append((f'engine_on_{key}', 'loop', dict(rpm=rpm, throttle=1.0, events=[ENGINE])))
    clips.append((f'engine_off_{key}', 'loop', dict(rpm=rpm, throttle=0.0, events=[ENGINE])))
clips.append(('limiter', 'loop', dict(rpm=18800, throttle=1.0, events=[ENGINE], note='rustyAC physics in neutral, throttle held down: the engine on its 18,800 rpm limiter (the F2004 bank has no limiter event)')))
clips.append(('wheelspin', 'loop', dict(events=[SKID, WHEEL], note='rear wheels at 110 rad/s with slip ratio 1, the speed the sound reads 20 km/h (full skid volume), engine level 0')))
clips.append(('pit_wheelgun', 'oneshot', dict(events=['event:/common/screw'])))
clips.append(('pit_wheelgun_off', 'oneshot', dict(events=['event:/common/unscrew'])))
clips.append(('backfire_1', 'oneshot', dict(throttle=0.0, events=['event:/cars/ks_ferrari_f2004/backfire_ext'])))
clips.append(('burnout_reference', 'full', dict(events=[ENGINE, SKID, WHEEL, 'event:/cars/ks_ferrari_f2004/gear_ext'], note='rustyAC physics at Spa: throttle held down with the brake at 0.6 from the spawn until 2 s after a rear tyre bursts; the listener moves with the car')))

# the common gain: the loudest clip peaks at -1 dBFS
made = {}
for name, kind, meta in clips:
    a = load(name)
    if kind == 'loop':
        made[name] = make_loop(a)
    elif kind == 'oneshot':
        made[name] = (trim_oneshot(a), None)
    else:
        # from the first block that sounds
        first = np.nonzero(np.abs(a).max(axis=1) > 1e-4)[0][0]
        made[name] = (a[first:].copy(), None)
loudest = max(made, key=lambda n: np.abs(made[n][0]).max())
gain = 10 ** (-1.0 / 20) / np.abs(made[loudest][0]).max()
print('the loudest clip is', loudest)
print(f'common gain {db(gain):.2f} dB')

manifest = {
    'rustyac_version': version,
    'format': {'sample_rate': SR, 'bits': 16, 'channels': 2},
    'common_gain_db': round(db(gain), 3),
    'common_gain_note': 'every raw clip (master level 0.12 in FMOD) was multiplied by this one gain: the loudest clip (' + loudest + ') then peaks at -1 dBFS',
    'listener': 'free camera 6 m from the car: 5.5 m to its right, 2.5 m behind its centre, 1.2 m up, looking at the car; no track sounds',
    'car': 'ks_ferrari_f2004',
    'bank': 'content/cars/ks_ferrari_f2004/sfx/ks_ferrari_f2004.bank (and content/sfx/common.bank)',
    'clips': [],
}
for name, kind, meta in clips:
    a, score = made[name]
    pcm = save(name, a * gain)
    entry = {'file': name + '.wav', 'kind': 'loop' if kind == 'loop' else 'oneshot', 'duration_s': round(len(pcm) / SR, 4), 'samples': int(len(pcm)), 'peak_dbfs': round(db(np.abs(pcm).max() / 32768.0), 2), 'rms_dbfs': round(db(np.sqrt((pcm.astype(np.float64) ** 2).mean()) / 32768.0), 2)}
    entry.update({k: v for k, v in meta.items() if k != 'events'})
    entry['fmod_events'] = meta['events']
    line = f"{name}: {entry['duration_s']} s, peak {entry['peak_dbfs']} dBFS, rms {entry['rms_dbfs']}"
    if kind == 'loop':
        across, p999, biggest = seam(pcm)
        centre, line_hz = pitch(a)
        entry.update({'loop_start': 0, 'loop_end': int(len(pcm)), 'loop_note': 'the whole file loops: the sample after the last is the first', 'seam_jump': across, 'jump_inside_p99_9': p999, 'jump_inside_max': biggest, 'spectral_centroid_hz': round(centre, 1), 'strongest_line_hz': round(line_hz, 1)})
        line += f', seam jump {across:.0f} (inside: 99.9 % below {p999:.0f}, max {biggest:.0f}), match {score:.3f}, centroid {centre:.0f} Hz, line {line_hz:.1f} Hz'
        assert across <= biggest, name
    assert np.abs(pcm).max() < 32767, name + ' clips'
    manifest['clips'].append(entry)
    print(line)
json.dump(manifest, open(os.path.join(out_dir, 'manifest.json'), 'w'), indent=2)
