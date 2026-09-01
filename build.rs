fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        slint_build::compile("ui/main.slint").expect("failed to compile the Slint user interface");
    }
}
