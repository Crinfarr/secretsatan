fn main() {
    println!(
        "cargo::rustc-env=SECSAT_BUILD_TIME={}",
        chrono::Utc::now().timestamp()
    );
}
