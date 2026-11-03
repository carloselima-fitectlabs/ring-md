use clap::Parser;
use gtk::glib;

fn main() -> glib::ExitCode {
    let cli = ringmd::cli::Cli::parse();
    ringmd::logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting RingMD");

    ringmd::app::run(cli)
}
