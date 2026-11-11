fn main() {
    kairos_tick::install_quiet_panic_hook();
    std::process::exit(kairos_cli::kairos_run_main());
}
