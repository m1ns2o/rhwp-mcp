#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if let Err(err) = rhwp::mcp::run_stdio() {
        eprintln!("rhwp-mcp: {err}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    eprintln!("rhwp-mcp is only available on native targets");
}
