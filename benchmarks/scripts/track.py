"""Generate benchmarks/data/track.csv: a synthetic railway track table.

The table copies the structure of a real track geometry export, with no
real data in it: two lines with two tracks each, one row per metre of
chainage (`pk`), and formulas that depend on a text column and on the
order of the rows:

- `curve_type` (Straight / Clothoid / Circular) decides the formula of
  `radius`, `cant` and `widening`: 0 on straights, constant per curve on
  circular arcs, and on clothoids a curvature (1 / radius) that grows
  linearly with `pk`, with `cant = K / radius` and `widening = W / radius`
  where K and W are those of the curve the clothoid leads into.
- `slope_type` (Gradient / Vertical curve) decides `gradient_pct`:
  constant per section on gradients and linear in `pk` on vertical curves;
  `z` is the height, rounded to the mm, and its slope is `gradient_pct`.
- `x`, `y` follow the track, one metre apart, so `pk` is the distance
  travelled along it.
- `key = line + "|" + track`, `profile_code` <-> `profile_name`, and the
  number in `asset_code` is `asset_id - 4835`.
- `accel` is noise: nothing explains it.

Run with Python 3 and no dependencies: python benchmarks/scripts/track.py
"""

import csv
import math
import os
import random

rng = random.Random(20261127)
PROFILES = [
    ("S140700", "Track circuit FTGS 917"),
    ("S140701", "Track circuit FTGS 46"),
    ("S140900", "Track circuit TI 21-M"),
    ("S140500", "Track circuit FS2500"),
]


def plan(length):
    """Curvature runs: (type, length, curvature at the end, cant, widening)."""
    runs, total = [], 0
    while total < length:
        straight = rng.randint(40, 160)
        runs.append(("Straight", straight, 0.0, 0.0, 0.0))
        radius = rng.choice([1, -1]) * rng.choice(
            [rng.uniform(90, 600), rng.uniform(600, 3000), rng.uniform(3000, 20000)]
        )
        cant = math.copysign(rng.uniform(0, 150), radius) if abs(radius) < 3000 else rng.uniform(-3, 3)
        widening = rng.uniform(0, 20) if abs(radius) < 600 else 0.0
        clothoid = rng.randint(15, 60)
        arc = rng.randint(25, 150)
        runs.append(("Clothoid", clothoid, 1 / radius, cant, widening))
        runs.append(("Circular", arc, 1 / radius, cant, widening))
        runs.append(("Clothoid", clothoid, -1 / radius, cant, widening))
        total += straight + 2 * clothoid + arc
    return runs


def vertical(length):
    """Gradient runs: (type, length, gradient at the start, at the end) in %."""
    runs, total, g = [], 0, rng.uniform(-3, 3)
    while total < length:
        n = rng.randint(80, 300)
        runs.append(("Gradient", n, g, g))
        g2 = rng.uniform(-4, 4)
        m = rng.randint(20, 80)
        runs.append(("Vertical curve", m, g, g2))
        g = g2
        total += n + m
    return runs


def track(line, number, start_pk, length, x0, y0, heading):
    rows = []
    curves = plan(length)
    # One entry per metre: (type, curvature, cant, widening).
    metres = []
    for kind, n, k_end, cant, widening in curves:
        for i in range(n):
            if kind == "Straight":
                metres.append((kind, 0.0, 0.0, 0.0))
            elif kind == "Circular":
                metres.append((kind, k_end, cant, widening))
            else:
                # The entry clothoid ramps up to the arc's curvature; the
                # exit one (stored with the sign flipped) ramps back down.
                entering = metres[-1][0] == "Straight" if i == 0 else entering
                t = (i + 0.5) / n
                if entering:
                    k = k_end * t
                    share = t
                else:
                    k = -k_end * (1 - t)
                    share = 1 - t
                metres.append((kind, k, cant * share, widening * share))
    metres = metres[:length]
    slopes = []
    for kind, n, g1, g2 in vertical(length):
        for i in range(n):
            slopes.append((kind, g1 + (g2 - g1) * (i + 0.5) / n))
    slopes = slopes[:length]

    x, y, z = x0, y0, rng.uniform(-20, 60)
    profile = rng.randrange(len(PROFILES))
    accel = 0.0
    asset = rng.randint(10000, 10400)
    for i, ((kind, k, cant, widening), (skind, g)) in enumerate(zip(metres, slopes)):
        heading += k  # one metre of track turns by k radians
        x += math.cos(heading)
        y += math.sin(heading)
        z += g / 100
        if rng.random() < 0.01:
            profile = rng.randrange(len(PROFILES))
        accel = round(rng.choice([-2, -1, 0, 1, 2]) * 0.7556 - 0.0729, 4)
        has_asset = rng.random() < 0.05
        if has_asset:
            asset += rng.randint(1, 4)
        rows.append(
            {
                "line": line,
                "track": number,
                "key": f"{line}|{number}",
                "pk": start_pk + i + 1,
                "curve_type": kind,
                "radius": round(1 / k, 3) if k else 0.0,
                "cant": round(cant, 6),
                "widening": round(widening, 6),
                "slope_type": skind,
                "gradient_pct": round(g, 4),
                "z": round(z, 3),
                "x": round(x, 3),
                "y": round(y, 3),
                "profile_code": PROFILES[profile][0],
                "profile_name": PROFILES[profile][1],
                "asset_id": asset if has_asset else "",
                "asset_code": f"SIG{asset - 4835}" if has_asset else "",
                "accel": accel,
            }
        )
    return rows


def main():
    rows = []
    for line, start in [("L1", 100000), ("L3", 2800)]:
        for number in ("1", "2"):
            x0, y0 = rng.uniform(428000, 436000), rng.uniform(4578000, 4590000)
            rows += track(line, number, start, 900, x0, y0, rng.uniform(0, 2 * math.pi))
    out = os.path.join(os.path.dirname(__file__), "..", "data", "track.csv")
    with open(out, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=list(rows[0]))
        w.writeheader()
        w.writerows(rows)
    print(f"{len(rows)} rows written to {os.path.normpath(out)}")


if __name__ == "__main__":
    main()
