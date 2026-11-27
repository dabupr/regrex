import math
from pathlib import Path

import pytest

import regrex

DATA = Path(__file__).resolve().parents[2] / "regrex-cli" / "tests"


def test_search_finds_kinetic_energy():
    m = [1.0, 2.5, 4.0, 0.8, 7.2, 3.3, 5.1, 9.0]
    v = [3.0, 1.2, 7.5, 12.0, 0.6, 4.4, 2.2, 5.0]
    e = [0.5 * a * b * b for a, b in zip(m, v)]
    best = regrex.search({"m": m, "v": v}, e, target_name="E")[0]
    assert best["formula"] == "0.5 * m * v^2"
    assert best["exact"]


def test_search_with_units():
    t = [0.5 + 0.3 * i for i in range(12)]
    v0 = [1.0 + (i * 7 % 5) for i in range(12)]
    d = [4.905 * x * x for x in t]
    best = regrex.search({"t": t, "v0": v0}, d, units={"t": "s", "v0": "m/s"}, target_unit="m", max_size=5)[0]
    assert best["formula"] == "4.905 * t^2"


def test_scan_file_reports_exceptions():
    report = regrex.scan_file(DATA / "invoices.csv", ignore=["invoice"])
    assert report["schema_version"] == 1
    vat = next(d for d in report["derived"] if d["column"] == "vat")
    assert vat["formula"] == "0.21 * net"
    assert vat["exception_rows"] == [42]
    assert vat["exceptions"][0]["diagnosis"]["description"] == "used 0.1 instead of 0.21"


def test_scan_file_reads_a_sqlite_table(tmp_path):
    import csv
    import sqlite3

    rows = list(csv.reader(open(DATA / "invoices.csv", newline="")))
    db = tmp_path / "shop.db"
    with sqlite3.connect(db) as conn:
        conn.execute("CREATE TABLE customers (id INTEGER, name TEXT)")
        conn.execute(f"CREATE TABLE invoices ({', '.join(rows[0])})")
        conn.executemany(f"INSERT INTO invoices VALUES ({', '.join('?' * len(rows[0]))})", rows[1:])
    conn.close()
    report = regrex.scan_file(db, sheet="invoices", ignore=["invoice"])
    vat = next(d for d in report["derived"] if d["column"] == "vat")
    assert vat["formula"] == "0.21 * net"


def test_scan_mapping_with_missing_values():
    price = [10.0, 5.5, 3.2, 8.0, 1.25, 99.9, 12.3, 7.0, 0.99, 45.0, 2.5, 18.75]
    qty = [2, 4, 1, 3, 10, 1, 5, 6, 12, 2, 8, 3]
    total = [p * q for p, q in zip(price, qty)]
    total[3] = None  # missing cell
    report = regrex.scan({"price": price, "qty": qty, "total": total})
    derived = {d["column"]: d for d in report["derived"]}
    assert derived["total"]["formula"] == "price * qty"
    assert derived["total"]["checked_rows"] == 11


def test_scan_pandas_dataframe():
    pd = pytest.importorskip("pandas")
    df = pd.DataFrame({"a": [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0], "label": list("abcdefg")})
    df["b"] = df["a"] * 3 + 1
    report = regrex.scan(df)
    assert [d["formula"] for d in report["derived"] if d["column"] == "b"] == ["3 * a + 1"]


def test_errors_are_value_errors():
    with pytest.raises(ValueError, match="unknown unit"):
        regrex.search({"x": [1.0, 2.0]}, [1.0, 2.0], units={"x": "parsecs"}, target_unit="m")
    with pytest.raises(ValueError, match="missing"):
        regrex.search({"x": [1.0, 2.0]}, [1.0, math.nan])
    with pytest.raises(TypeError):
        regrex.scan([1, 2, 3])


def test_scan_tolerates_rounded_values_like_scan_file():
    import csv

    rows = list(csv.DictReader(open(DATA / "invoices.csv", newline="")))
    data = {name: [float(r[name]) for r in rows] for name in rows[0] if name not in ("invoice", "product")}
    from_file = regrex.scan_file(DATA / "invoices.csv", ignore=["invoice"])
    from_data = regrex.scan(data)
    vat_file = next(d for d in from_file["derived"] if d["column"] == "vat")
    vat_data = next(d for d in from_data["derived"] if d["column"] == "vat")
    assert vat_data["formula"] == vat_file["formula"] == "0.21 * net"
    assert vat_data["matched_rows"] == vat_file["matched_rows"]
    assert vat_data["exception_rows"] == vat_file["exception_rows"] == [42]


def test_scan_takes_decimals_by_hand():
    net = [10.0 + 7.3 * i for i in range(40)]
    vat = [round(0.21 * x, 1) for x in net]
    report = regrex.scan({"net": net, "vat": vat}, decimals={"vat": 1})
    vat_row = next(d for d in report["derived"] if d["column"] == "vat")
    assert vat_row["formula"] == "0.21 * net"
    assert vat_row["exception_rows"] == []


def _track(tmp_path):
    """A small track table and a second file of designed radii, matched on
    line and chainage."""
    a = ["line,pk,curve,radius"]
    b = ["line,pk,design_radius"]
    for line in ("L1", "L2"):
        for pk in range(0, 400, 5):
            curve = "circular" if 100 <= pk < 300 else "straight"
            r = 500 + (pk // 100) * 100 if curve == "circular" else 0
            a.append(f"{line},{pk},{curve},{r}")
            b.append(f"{line},{pk},{r}")
    pa, pb = tmp_path / "track.csv", tmp_path / "design.csv"
    pa.write_text("\n".join(a) + "\n")
    pb.write_text("\n".join(b) + "\n")
    return pa, pb


def test_scan_file_with_a_second_file(tmp_path):
    a, b = _track(tmp_path)
    report = regrex.scan_file(a, with_=b)
    assert report["joined"]["keys"] == ["line", "pk"]
    assert report["joined"]["matched_rows"] == report["rows"] == 160
    copy = next(d for d in report["derived"] if d["column"] == "design_radius")
    assert copy["copy_of"] == "radius"


def test_quick_scan_file():
    report = regrex.scan_file(DATA / "invoices.csv", ignore=["invoice"], max_size=3, quick=True)
    assert report["quick"]["sample_rows"] == 60
    assert report["quick"]["dropped"] == []
    assert {d["column"] for d in report["derived"]} >= {"price", "subtotal"}


def test_describe_file_gives_the_structure(tmp_path):
    a, _ = _track(tmp_path)
    d = regrex.describe_file(a)
    assert d["describe_schema_version"] == 1
    assert d["rows"] == 160
    assert d["structure"]["groups"] == ["line"]
    assert d["structure"]["order"] == "pk"
    kinds = {c["name"]: c["kind"] for c in d["columns"]}
    assert kinds["curve"] == "text" and kinds["radius"] == "number"
