use clap::Parser;

fn main() {
    let cli = bt_cm749::cli::Cli::parse();
    if let Err(err) = bt_cm749::cli::run(cli) {
        eprintln!("ERROR: {err}");
        std::process::exit(err.exit_code());
    }
}
