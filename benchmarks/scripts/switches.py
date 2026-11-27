"""Generate benchmarks/data/switches.csv: switches placed on the synthetic
track of track.csv by their coordinates only.

Each switch is given as a WKT point a few centimetres off the track, with
the key of its track and its chainage from a catalogue. The file shares no
column with track.csv, so `regrex scan track.csv --with switches.csv` has
to match each switch to the nearest track point to find that its
`track_key` is the track's `key` and its `pk_catalogue` the track's `pk`.

Run after track.py, with Python 3 and no dependencies:
python benchmarks/scripts/switches.py
"""

import csv
import os
import random

rng = random.Random(20261128)
here = os.path.dirname(__file__)
with open(os.path.join(here, "..", "data", "track.csv"), newline="") as f:
    track = list(csv.DictReader(f))

rows = []
for n, r in enumerate(sorted(rng.sample(range(len(track)), 40))):
    t = track[r]
    dx, dy = rng.uniform(-0.15, 0.15), rng.uniform(-0.15, 0.15)
    rows.append(
        {
            "switch": f"AG-{101 + n}",
            "track_key": t["key"],
            "pk_catalogue": t["pk"],
            "geom": f"POINT ({float(t['x']) + dx:.3f} {float(t['y']) + dy:.3f})",
        }
    )
out = os.path.join(here, "..", "data", "switches.csv")
with open(out, "w", newline="") as f:
    w = csv.DictWriter(f, fieldnames=list(rows[0]))
    w.writeheader()
    w.writerows(rows)
print(f"{len(rows)} switches written to {os.path.normpath(out)}")
