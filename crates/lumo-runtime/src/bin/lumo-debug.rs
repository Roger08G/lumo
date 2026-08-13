fn main() {
    if let Err(error) = lumo_runtime::cli::debug::run() {
        lumo_runtime::cli::print_failure_and_exit(error);
    }
}
