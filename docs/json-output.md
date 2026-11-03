# JSON output

`regrex scan --json`, `regrex fit --json`, the Python package and the MCP server all return the same JSON. Every report has a `schema_version`; it is `1` today. Within a version, fields may be added but existing fields keep their name and meaning, so programs can rely on them.

Rows are counted from 1, starting at the first row after the header, as a spreadsheet user would count data rows. Numbers that are not finite are written as `null`.

## Scan report

Returned by `regrex scan --json`, `regrex.scan()`, `regrex.scan_file()` and the `scan_dataset` tool.

```json
{
  "schema_version": 1,
  "file": "examples/data/invoices.csv",
  "rows": 60,
  "columns": ["price", "quantity", "discount_pct", "subtotal", "net", "vat", "total"],
  "skipped_columns": ["product"],
  "derived": [
    {
      "column": "vat",
      "formula": "0.21 * net",
      "inputs": ["net"],
      "complexity": 3,
      "matched_rows": 59,
      "checked_rows": 60,
      "match_ratio": 0.9833333333333333,
      "exception_rows": [42],
      "exceptions": [{ "row": 42, "actual": 29.97, "expected": 62.937 }]
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `file` | The file that was read (`<data>` for in-memory tables). |
| `rows` | Number of data rows. |
| `columns` | Numeric columns that were analysed. |
| `skipped_columns` | Columns left out because they are not numeric. |
| `derived[].column` | A column that is computed from others. |
| `derived[].formula` | Its formula, written with column names. |
| `derived[].inputs` | Columns the formula uses. |
| `derived[].complexity` | Weighted size of the formula; smaller is simpler. |
| `derived[].matched_rows` | Rows that follow the formula, within the rounding of the column. |
| `derived[].checked_rows` | Rows with no missing value in the column or its inputs. |
| `derived[].match_ratio` | `matched_rows / checked_rows`. |
| `derived[].exception_rows` | Every row that breaks the formula. |
| `derived[].exceptions` | Value in the file and value the formula gives, for the first 100 exceptions. |

The `check_formula` MCP tool returns one `derived` entry for the formula it was given, even when few rows match.

## Fit report

Returned by `regrex fit --json` and the `fit_formula` and `fit_values` tools; `regrex.search()` returns its `candidates` list.

```json
{
  "schema_version": 1,
  "target": "E",
  "inputs": ["m", "v"],
  "rows": 8,
  "candidates": [
    { "formula": "0.5 * m * v^2", "complexity": 7, "mse": 0.0, "r2": 1.0, "max_abs_error": 0.0, "exact": true }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `target` | The column explained. |
| `inputs` | Columns that formulas may use. |
| `rows` | Rows used; rows with missing values are left out. |
| `candidates` | A Pareto front from simplest to most accurate: each formula is more accurate than every simpler one. |
| `candidates[].formula` | The formula, with fitted constants. |
| `candidates[].complexity` | Weighted size; smaller is simpler. |
| `candidates[].mse` | Mean squared error. |
| `candidates[].r2` | Coefficient of determination (1 is a perfect fit). |
| `candidates[].max_abs_error` | Largest absolute error over the rows. |
| `candidates[].exact` | Whether the formula reproduces the data up to floating point error. |

## Formula syntax

Formulas use `+ - * / ^`, parentheses, `sqrt`, `exp`, `log` (natural logarithm, also `ln`), `sin`, `cos`, and the constants `pi` (`π`) and `e`. `x^2` is also written `x²`. `-x^2` means `-(x^2)`. The same syntax is accepted by `check_formula` and `check_units`.
