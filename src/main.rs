#[cfg(windows)]
fn main() {
    if let Err(error) = lastkey::platform::windows::run() {
        eprintln!("LastKey failed to start: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LastKey Windows input support is not available on this platform.");
    std::process::exit(1);
}
