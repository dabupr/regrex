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
  "converted_columns": [],
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
      "other": null,
      "copy_of": null
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
| `converted_columns` | Columns whose cells are not plain numbers but were read as numbers: `column` (as written in the file) and `read_as`, one of `true/false as 1/0`, `times as minutes`, `dates as days since 1970-01-01`, `dates and times as minutes since 1970-01-01` or `numbers in <unit>`. |
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
| `….diagnosis.kind` | One of `rounding`, `typo`, `sign`, `factor`, `wrong_constant`, `wrong_variable`, `swapped_variables`, `missing_operation`, `missing_term`, `wrong_operation`, `swapped_operands`, `other_cell`, `midnight`, `zero`, `other_formula`. New kinds may be added. |
| `….diagnosis.description` | What went wrong, in English. |
| `….diagnosis.formula` | The formula the row follows instead, when the mistake is in the formula. |

| `derived[].other` | A second formula followed by many of the exceptions, when the column mixes two kinds of rows, or `null`: its `formula`, the `rows` that follow it and the `split` rule that tells them apart (`column`, `rule` such as `year >= 2023`, `accuracy`), or `null` when no column does. |
| `derived[].copy_of` | When the column is a copy of another one (the same values, maybe shifted, negated or scaled by a power of ten, 60 or 3600, such as `serial + 1` or `1000 * km`), that column; otherwise `null`. Copies are found before the search, are not used as inputs, and the text report lists them apart under "Copies of other columns". |
| `near_misses[]` | Formulas the scan found but did not report, most rows matched first (at most 3 per column): where to look next. Each has `column`, `formula`, `inputs`, `complexity`, `matched_rows`, `checked_rows`, `match_ratio`; `reason`, why it was not reported: `coverage` (too few rows match), `usual_value` (right mostly where the column has its most common value, often 0) `no_split_rule` (it fits the rows that break the column's first formula, but no other column tells them apart) or `other_values` (the column holds one value, often 0, on most rows, and the formula is for the other rows: `checked_rows` counts only those); `reason_text`, the same in words; `holds_when`, a rule on another column that picks out the rows where it holds (`column`, `rule`, `accuracy`) or `null`; `regime`, the regime it was searched within, or `null`; `missed_count`, the checked rows where it fails, and `missed_rows`, the first 20 of them. Only formulas that hold on at least half of the rows they were checked on (and of the rows that differ from the column's most common value) are kept. |
| `not_found[]` | Each column that was searched without finding a formula: `column`, and `inputs_tried`, the columns tried as its inputs (the ones that predict it best, at most 5 and sometimes one more). A formula over any other column cannot have been found. Constants, flags (two values) and copies are not searched, so they are not listed. |
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

## Agent report

Returned by `regrex scan --agent` and the `scan_dataset` tool with `"format": "agent"`. The scan report above is written for a person; this one is for an AI agent that judges the results: whether a formula makes sense for what the columns hold, whether a near miss is a real rule of part of the data, which columns to search again. It is compact JSON on one line, with its own `agent_schema_version` (`1` today), and holds everything needed to judge without searching again. Regrex computes all of it; it does not ask an AI.

| Field | Meaning |
| --- | --- |
| `columns[]` | Every column of the file, in order: `name`; `kind`, `number` or `text`; `read_as` when the cells were converted (`times as minutes`); `distinct` values (counted up to 1000); `missing` cells; `min` and `max` for numbers; `increasing` (a row number, an ID or a distance along a line); `searched`, and `not_searched` when it was not: `text`, `constant`, `flag` (two values), `copy` or `not_requested`. |
| `found[]` | Each formula: `column`, `formula`, `inputs`, `complexity`, `matched_rows`, `checked_rows`; `exact`, whether it gives the exact values rather than values within rounding; `max_error` and `median_error`, the largest and median absolute difference on the matched rows; `copy_of`; `known`, the id of the library formula it is; `other`, a second formula with its number of `rows` and the rule (`when`) that picks them out; `wrong_count`, the rows that follow neither, and `wrong`, the first 10 with `actual`, `expected` and the `likely` mistake. |
| `near_misses[]` | As in the scan report, with `holds_when` as the rule's text and at most 10 `missed_rows`. |
| `unexplained[]` | Columns searched without a formula, with the `inputs_tried`. |
| `notes` | A few sentences on how to read the report. |

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

Formulas use `+ - * / ^`, parentheses, `sqrt`, `exp`, `log` (natural logarithm, also `ln`), `sin`, `cos`, the integer division `div` (also `//`, and `floor(a / b)`) and the remainder `mod` (also `%`), and the constants `pi` (`π`) and `e`. `x^2` is also written `x²`. `-x^2` means `-(x^2)`. The same syntax is accepted by `check_formula` and `check_units`.
