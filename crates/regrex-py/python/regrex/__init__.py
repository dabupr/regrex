"""Find the formulas behind your data.

Regrex is a symbolic regression engine written in Rust. This package exposes
three functions:

* :func:`search` finds formulas for one target from input columns;
* :func:`scan` finds which columns of a table are computed from the others;
* :func:`scan_file` does the same for a file: CSV, spreadsheet, JSON,
  Parquet, Arrow or SQLite.

Results are plain dictionaries with the same fields as the JSON output of the
``regrex`` command-line tool (``schema_version`` 1).
"""

from __future__ import annotations

import json
import math
from typing import Any, Mapping, Optional, Sequence

from . import _native

__all__ = ["search", "scan", "scan_file", "__version__"]
__version__: str = _native.__version__


def _float(value: Any) -> float:
    """Converts a cell to float; missing or non-numeric values become NaN."""
    try:
        f = float(value)
    except (TypeError, ValueError):
        return math.nan
    return f


def _columns(data: Any) -> "dict[str, list[float]]":
    """Accepts a pandas/polars DataFrame or a mapping of name -> values."""
    if hasattr(data, "columns") and not isinstance(data, Mapping):
        # pandas and polars DataFrames
        out = {}
        for name in data.columns:
            values = [_float(v) for v in list(data[name])]
            # Skip text columns: keep columns where most values are numbers.
            numeric = sum(1 for v in values if not math.isnan(v))
            if values and numeric >= 0.9 * len(values):
                out[str(name)] = values
        return out
    if isinstance(data, Mapping):
        return {str(k): [_float(v) for v in vs] for k, vs in data.items()}
    raise TypeError("expected a DataFrame or a mapping of column name to values")


def search(
    inputs: Any,
    target: Sequence[float],
    *,
    target_name: str = "y",
    max_size: int = 7,
    max_params: int = 1,
    max_results: int = 10,
    units: Optional[Mapping[str, str]] = None,
    target_unit: Optional[str] = None,
) -> "list[dict[str, Any]]":
    """Finds formulas ``target ≈ f(inputs)``, best first.

    ``inputs`` is a DataFrame or a mapping of column name to values. Each
    result has ``formula``, ``complexity``, ``mse``, ``r2``,
    ``max_abs_error`` and ``exact``. Give ``units`` (for example
    ``{"t": "s", "v": "m/s"}``) and ``target_unit`` to rule out formulas that
    mix incompatible units.
    """
    cols = _columns(inputs)
    y = [_float(v) for v in target]
    if any(math.isnan(v) for v in y):
        raise ValueError("the target has missing or non-numeric values")
    names = list(cols)
    unit_list = [units.get(n) for n in names] if units else None
    report = _native.search_json(
        names,
        [cols[n] for n in names],
        y,
        target_name,
        max_size,
        max_params,
        max_results,
        unit_list,
        target_unit,
    )
    return json.loads(report)["candidates"]


def scan(
    data: Any,
    *,
    ignore: Sequence[str] = (),
    units: Optional[Mapping[str, str]] = None,
    min_match: float = 0.9,
    max_size: int = 5,
    decimals: Optional[Mapping[str, int]] = None,
) -> "dict[str, Any]":
    """Finds the columns of ``data`` that are computed from other columns.

    ``data`` is a DataFrame or a mapping of column name to values. Returns a
    report whose ``derived`` list holds, for each derived column, its
    ``formula``, ``matched_rows``, ``checked_rows`` and the ``exception_rows``
    that break it (counted from 1).

    Values rounded to a few decimals (``vat = round(0.21 * net, 2)``) match
    within half a unit of the last decimal. The decimals of each column are
    inferred from its values; ``decimals`` (for example ``{"vat": 2}``) sets
    them by hand.
    """
    cols = _columns(data)
    names = [n for n in cols if n not in set(ignore)]
    unit_list = [units.get(n) for n in names] if units else None
    decimal_list = [decimals.get(n) for n in names] if decimals else None
    report = _native.scan_json(
        names,
        [cols[n] for n in names],
        unit_list,
        min_match,
        max_size,
        "<data>",
        decimal_list,
    )
    return json.loads(report)


def scan_file(
    path: str,
    *,
    sheet: Optional[str] = None,
    ignore: Sequence[str] = (),
    min_match: float = 0.9,
    max_size: int = 5,
) -> "dict[str, Any]":
    """Like :func:`scan`, for a file: CSV, a spreadsheet (.xlsx, .xls, .ods),
    JSON (.json, .ndjson, .jsonl), Parquet (.parquet), Arrow (.arrow,
    .feather) or a SQLite database (.sqlite, .db).

    ``sheet`` picks the sheet of a spreadsheet or the table of a database
    (default: the first one). Units can be given in the headers, as in
    ``time (s)``.
    """
    report = _native.scan_file_json(str(path), sheet, list(ignore), min_match, max_size)
    return json.loads(report)
