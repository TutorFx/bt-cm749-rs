use clap::Parser;

fn main() {
    let cli = bt_cm749::cli::Cli::parse();
    if let Err(err) = bt_cm749::cli::run(cli) {
        if !matches!(err, bt_cm749::error::Error::Shown(_)) {
            eprintln!("ERROR: {err}");
        }
        std::process::exit(err.exit_code());
    }
}
