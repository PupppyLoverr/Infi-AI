//! The interface (ui/, Luau) compiled into the binary, as zgx's terminal
//! example ships with `terminal-bundle`.
fn main() {
    println!("cargo::rerun-if-changed=ui");
    zgx_script::build::bundle("ui", "terminal").expect("bundle the terminal's scripts");
}
