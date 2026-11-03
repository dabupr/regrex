# regrex (Python)

Find the formulas behind your data. Python bindings for [Regrex](https://github.com/dabupr/regrex), a symbolic regression engine written in Rust.

```python
import regrex

m = [1.0, 2.5, 4.0, 0.8, 7.2, 3.3]
v = [3.0, 1.2, 7.5, 12.0, 0.6, 4.4]
energy = [0.5 * a * b * b for a, b in zip(m, v)]

best = regrex.search({"m": m, "v": v}, energy)[0]
print(best["formula"])  # 0.5 * m * v^2

# Which columns of a table are computed from the others?
report = regrex.scan_file("invoices.csv", ignore=["invoice"])
for d in report["derived"]:
    print(d["column"], "=", d["formula"], d["exception_rows"])

# Also works with pandas DataFrames or dicts of columns:
report = regrex.scan(df, units={"time": "s", "distance": "m"})
```

Results are plain dictionaries with the same fields as the `--json` output of the `regrex` command-line tool.
