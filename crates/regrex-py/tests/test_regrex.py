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
