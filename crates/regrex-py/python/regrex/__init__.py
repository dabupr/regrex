"""Find the formulas behind your data.

Regrex is a symbolic regression engine written in Rust. This package exposes
three functions:

* :func:`search` finds formulas for one target from input columns;
* :func:`scan` finds which columns of a table are computed from the others;
* :func:`scan_file` does the same for a file: CSV, spreadsheet, JSON,
  Parquet, Arrow or SQLite, optionally matched with a second file;
* :func:`describe_file` tells what a file holds (column roles, groups, the
  order of the rows, relations between text columns) without searching.

Results are plain dictionaries with the same fields as the JSON output of the
``regrex`` command-line tool (``schema_version`` 1). The :class:`ScanReport`
and :class:`Description` types describe their main fields for type checkers.
"""

from __future__ import annotations

import json
import math
from typing import Any, List, Mapping, Optional, Sequence, TypedDict

from . import _native

__all__ = [
    "search",
    "scan",
    "scan_file",
    "describe_file",
    "ScanReport",
    "Description",
    "Structure",
    "Role",
    "TextRelation",
    "Joined",
    "Quick",
    "__version__",
]
__version__: str = _native.__version__


class Role(TypedDict, total=False):
    """What one column is for."""

    column: str
    #: ``measurement``, ``regime``, ``group key``, ``order``, ``code``,
    #: ``identifier``, ``flag`` or ``constant``.
    role: str
    category: bool
    distinct: int
    #: Average rows a value holds along the order, when there is one.
    mean_run: float


class Structure(TypedDict):
    """How a table is laid out."""

    #: Columns that split the rows into independent sequences (a line, a track).
    groups: List[str]
    #: The column that orders the rows within each group (a chainage, a time).
    order: Optional[str]
    roles: List[Role]


class TextRelation(TypedDict):
    """A relation between text columns, such as ``key = line + "|" + track``."""

    column: str
    formula: str
    inputs: List[str]
    matched_rows: int
    checked_rows: int


class Joined(TypedDict):
    """How the rows of a second file were matched with those of the first."""

    file: str
    name: str
    keys: List[str]
    matched_rows: int
    rows: int
    repeated_rows: int


class Quick(TypedDict):
    """What a quick scan searched on a sample of rows only."""

    sample_rows: int
    max_broken_share: float
    not_searched: List[str]
    dropped: List[str]


class _ScanReport(TypedDict):

    schema_version: int
    file: str
    rows: int
    columns: List[str]
    skipped_columns: List[str]
    category_columns: List[str]
    structure: Structure
    converted_columns: List[dict]
    derived: List[dict]
    not_found: List[dict]
    near_misses: List[dict]


class ScanReport(_ScanReport, total=False):
    """The result of :func:`scan` and :func:`scan_file`; see
    ``docs/json-output.md`` for every field. ``text_relations``, ``quick``
    and ``joined`` are there only when they apply."""

    text_relations: List[TextRelation]
    quick: Quick
    joined: Joined


class _Description(TypedDict):

    describe_schema_version: int
    file: str
    rows: int
    #: Every column: ``name``, ``kind`` (``number`` or ``text``), ``distinct``,
    #: ``missing``, ``min``, ``max``, ``read_as``, ``increasing``, and whether
    #: a scan would search it (``searched``, ``not_searched``).
    columns: List[dict]
    structure: Structure
    converted_columns: List[dict]
    text_relations: List[TextRelation]


class Description(_Description, total=False):
    """The result of :func:`describe_file`; ``joined`` is there only when a
    second file was given."""

    joined: Joined


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
) -> ScanReport:
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
    with_: Optional[str] = None,
    quick: bool = False,
    sample: Optional[int] = None,
) -> ScanReport:
    """Like :func:`scan`, for a file: CSV, a spreadsheet (.xlsx, .xls, .ods),
    JSON (.json, .ndjson, .jsonl), Parquet (.parquet), Arrow (.arrow,
    .feather) or a SQLite database (.sqlite, .db).

    ``sheet`` picks the sheet of a spreadsheet or the table of a database
    (default: the first one). Units can be given in the headers, as in
    ``time (s)``.

    ``with_`` is a second file whose rows are matched with those of the
    first on the columns both have (text or whole numbers, such as a line,
    a track and a chainage), so relations between the two files are found;
    the report's ``joined`` tells how the rows were matched.

    ``quick=True`` searches every column on a sample of ``sample`` rows
    (default: 5% of the rows, at least 2000) and confirms on all rows only
    the formulas found there: several times faster on large files. The
    report's ``quick`` lists what was searched on the sample only.
    """
    report = _native.scan_file_json(
        str(path),
        sheet,
        list(ignore),
        min_match,
        max_size,
        None if with_ is None else str(with_),
        quick,
        sample,
    )
    return json.loads(report)


def describe_file(
    path: str,
    *,
    sheet: Optional[str] = None,
    with_: Optional[str] = None,
) -> Description:
    """Tells what a file holds without searching for formulas, in about the
    time it takes to read it: every column with its values and range, how
    the table is laid out (``structure``: groups, the order of the rows and
    the role of each column) and the relations between text columns.

    Use it on a large or unknown file to decide which columns to scan.
    ``sheet`` and ``with_`` are as in :func:`scan_file`.
    """
    report = _native.describe_file_json(
        str(path), sheet, None if with_ is None else str(with_)
    )
    return json.loads(report)
