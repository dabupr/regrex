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
      "exceptions": [
        {
          "row": 42,
          "actual": 29.97,
          "expected": 62.937,
          "diagnosis": {
            "kind": "wrong_constant",
            "description": "used 0.1 instead of 0.21",
            "formula": "0.1 * net"
          }
        }
      ],
      "known": {
        "id": "tax_amount",
        "name": "Tax amount",
        "domain": "finance",
        "description": null,
        "expression": "p * t",
        "output": { "symbol": "tax", "name": "tax amount", "unit": "currency", "column": "vat", "value": null },
        "variables": [
          { "symbol": "p", "name": "net amount", "unit": "currency", "column": "net", "value": null },
          { "symbol": "t", "name": "tax rate", "unit": "1", "column": null, "value": 0.21 }
        ],
        "changed_constant": null,
        "exact": false,
        "source_title": "Value-added tax (Wikipedia)",
        "source_url": "https://en.wikipedia.org/wiki/Value-added_tax"
      },
      "other": null
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
| `derived[].exceptions[].diagnosis` | The most likely mistake, or `null` when no simple mistake reproduces the value. |
| `….diagnosis.kind` | One of `rounding`, `typo`, `sign`, `factor`, `wrong_constant`, `wrong_variable`, `swapped_variables`, `missing_operation`, `missing_term`, `wrong_operation`, `swapped_operands`, `other_cell`, `other_formula`. New kinds may be added. |
| `….diagnosis.description` | What went wrong, in English. |
| `….diagnosis.formula` | The formula the row follows instead, when the mistake is in the formula. |

| `derived[].other` | A second formula followed by many of the exceptions, when the column mixes two kinds of rows, or `null`: its `formula`, the `rows` that follow it and the `split` rule that tells them apart (`column`, `rule` such as `year >= 2023`, `accuracy`), or `null` when no column does. |
| `derived[].known` | The formula from the [library of known formulas](https://dabupr.github.io/regrex/library.html) that the column follows, or `null`. |
| `known.id`, `known.name`, `known.domain` | Which library formula it is; see `crates/regrex-formulas/library/`. |
| `known.expression` | The library formula, in its own symbols. |
| `known.output`, `known.variables[]` | Each symbol with its name and unit, and the `column` it was matched to, or the constant `value` the data uses for it (the tax rate above). |
| `known.changed_constant` | When the data follows the formula with a different constant, such as `g = 10` instead of `9.80665`: its `id`, `library_value` and `value`. |
| `known.exact` | Whether every variable is a column and no constant changed. |
| `known.source_title`, `known.source_url` | Where the formula comes from. |

A formula is only named when the column names agree with the library (`price`, `vat`, `velocity`…) or the formula is specific enough not to be a coincidence, so `a * b` alone is not called "line total".

A diagnosis is never a guess: Regrex recomputes the row with the mistake and only reports it when it gives the value in the file, within the rounding of the column. `other_cell` means the value is fine but another cell of the row is wrong, for example a wrong total that also breaks `net = total - vat`.

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
    { "formula": "0.5 * m * v^2", "complexity": 7, "mse": 0.0, "r2": 1.0, "max_abs_error": 0.0, "exact": true, "known": { "id": "kinetic_energy", "name": "Kinetic energy", "…": "…" } }
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
| `candidates[].known` | The known formula it matches, or `null`; same fields as in a scan report. |

## Formula syntax

Formulas use `+ - * / ^`, parentheses, `sqrt`, `exp`, `log` (natural logarithm, also `ln`), `sin`, `cos`, and the constants `pi` (`π`) and `e`. `x^2` is also written `x²`. `-x^2` means `-(x^2)`. The same syntax is accepted by `check_formula` and `check_units`.
