// Builds the Rust engine for the browser and generates the JavaScript glue
// in src/wasm/. Needs the wasm32-unknown-unknown target and wasm-bindgen-cli
// (same version as the wasm-bindgen crate in Cargo.lock).
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const target = process.env.CARGO_TARGET_DIR ?? join(root, "target");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit" });

run("cargo", ["build", "--release", "-p", "regrex-wasm", "--target", "wasm32-unknown-unknown"]);
run("wasm-bindgen", [
  join(target, "wasm32-unknown-unknown", "release", "regrex_wasm.wasm"),
  "--target", "web",
  "--out-dir", join(root, "web", "src", "wasm"),
]);
