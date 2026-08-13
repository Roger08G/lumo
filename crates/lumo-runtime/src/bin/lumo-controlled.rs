fn main() {
    if let Err(error) = lumo_runtime::cli::controlled::run() {
        lumo_runtime::cli::print_failure_and_exit(error);
    }
}
