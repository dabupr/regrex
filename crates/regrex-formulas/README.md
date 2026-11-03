# regrex-formulas

A library of well-known formulas and constants, grouped by domain, for [Regrex](https://github.com/dabupr/regrex).

Regrex uses it to give a **"known formula" badge** to a candidate found in the data (when it is equivalent to a formula of the library) and to build the **prior of each domain** (which operators and constants are common in physics, finance, etc.).

| Domain | File | What it covers |
|---|---|---|
| Mathematics | [`mathematics.toml`](library/mathematics.toml) | geometry, algebra, series, statistics, probability |
| Physics | [`physics.toml`](library/physics.toml) | mechanics, gravitation, fluids, thermodynamics, waves, optics, electromagnetism, modern physics |
| Chemistry | [`chemistry.toml`](library/chemistry.toml) | amounts, solutions, gases, kinetics, equilibrium, electrochemistry |
| Electronics and signals | [`electronics.toml`](library/electronics.toml) | DC/AC circuits, components, filters, decibels, noise, sampling |
| Finance and business | [`finance.toml`](library/finance.toml) | prices, VAT, margins, interest, loans, returns, depreciation |
| Computing | [`computing.toml`](library/computing.toml) | data sizes, transfer, performance laws, queues, reliability, information theory |
| Health and sport | [`health.toml`](library/health.toml) | BMI, metabolic rate, heart rate, blood pressure, nutrition, strength |
| Business and operations | [`business.toml`](library/business.toml) | sales and marketing KPIs, accounting ratios, inventory, operations, payroll, HR, hospitality |
| Energy and environment | [`energy.toml`](library/energy.toml) | electricity use and cost, solar, wind and hydro power, heating, batteries, fuels, CO₂ |
| Unit conversions | [`conversions.toml`](library/conversions.toml) | temperature, length, mass, speed, energy, pressure, angle |

Physical constants are the CODATA 2022 values published by NIST.

## Use from Rust

```rust
use regrex_formulas::Library;

let lib = Library::builtin();
let f = lib.formula("kinetic_energy").unwrap();
println!("{} = {}", f.output.symbol, f.expr_text); // E_k = 0.5 * m * v^2
println!("{}", f.eval(&[2.0, 3.0]));               // 9

// Is a formula found in the data the same as this one?
assert!(f.equivalent_to(|x| x[0] * x[1] * x[1] / 2.0, 1e-9));

// Raw material for the physics prior.
let ops = lib.operator_counts(Some("physics"));
let constants = lib.constants_used_in("physics");
```

The data files are embedded in the binary with `include_str!`, and the only dependencies are `serde` and `toml`, so the crate works in WebAssembly.

## Add a formula

Add a `[[formula]]` block to the file of its domain:

```toml
[[formula]]
id = "kinetic_energy"                      # unique, snake_case
name = "Kinetic energy"
description = "Optional longer text."
expr = "0.5 * m * v^2"
output = { symbol = "E_k", name = "kinetic energy", unit = "J" }
variables = [
  { symbol = "m", name = "mass", unit = "kg", aliases = ["mass"] },
  { symbol = "v", name = "speed", unit = "m/s", aliases = ["speed", "velocity"] },
]
tags = ["mechanics", "energy"]             # snake_case, free-form
source = { title = "Kinetic energy (Wikipedia)", url = "https://en.wikipedia.org/wiki/Kinetic_energy" }
checks = [{ inputs = [2, 3], output = 9 }] # worked examples, inputs in variable order
```

Then run `cargo test -p regrex-formulas`. The same test runs in CI and lists every problem it finds.

**Expressions** use calculator notation: `+ - * / ^`, unary `-`, parentheses and the functions `sqrt cbrt exp log ln log10 log2 sin cos tan asin acos atan atan2 sinh cosh tanh abs floor ceil` (`log` is the natural logarithm, like `ln`). Names are the variables of the formula (Unicode is fine: `θ`, `ρ`, `ΔT`) or library constants by id (`pi`, `e`, `g`, `c`, `h`, `k_B`, `N_A`, `R`, `G`...). A variable hides a constant with the same name.

**Units** are written like `kg`, `m/s^2`, `J/(mol K)`, `N m`, `kW h` or `1` for plain numbers. SI prefixes work (`km`, `mg`, `µs`, `GB`), and so do a few common non-SI units for conversions (`min`, `h`, `L`, `mi`, `lb`, `degC`, `degF`, `kcal`, `atm`...). `currency` is the dimension of money and `bit`/`B` are dimensionless. The full table is in [`src/units.rs`](src/units.rs).

**What is checked**

- the TOML has no unknown or missing fields, and ids are unique;
- every name resolves, every declared variable is used, every unit parses;
- both sides have the same dimension (plain numbers in a sum adapt, so `1.8 * T_C + 32` passes; `exp`, `log` and trigonometric functions need dimensionless arguments);
- formulas that use a constant with a dimension (`g`, `c`...) use coherent SI units, since constants are stored in SI;
- every `checks` example gives the stated result (relative error below 10⁻⁶);
- formulas tagged `unit_conversion` agree with the unit table (except temperature scales, tagged `affine`);
- sources have a title, and links use `https://`.

Empirical formulas whose coefficients carry units (for example Mifflin–St Jeor, `10 * m + 6.25 * h - 5 * age + 5`) declare their variables as `1` and say the expected unit in the name.

## Add a constant

```toml
[[constant]]
id = "g"                 # name used in formulas
symbol = "g_n"           # optional display symbol
name = "standard acceleration of gravity"
value = 9.80665
unit = "m/s^2"           # coherent SI only
exact = true             # or give `uncertainty = ...`
source = { title = "CODATA 2022 (NIST)", url = "https://physics.nist.gov/cgi-bin/cuu/Value?gn" }
```

## License

MIT
