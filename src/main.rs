use clap::{App, AppSettings, Arg, SubCommand};
use slog::Drain;

mod config;
mod engines;
mod tui;

const DEFAULT_CONFIG_FILE: &str = "curtis.toml";
const VERSION: &str = "0.1.0";

fn main() {
    // Log
    let decorator = slog_term::TermDecorator::new().build();
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let drain = slog_envlogger::new(drain);
    let drain = slog_async::Async::new(drain).build_no_guard();
    let drain = drain.fuse();

    let logger = slog::Logger::root(drain, slog::o!());
    let _guard = slog_scope::set_global_logger(logger);
    slog_stdlog::init().unwrap();

    // Cmdline
    let args = App::new("Curtis")
        .version(VERSION)
        .setting(AppSettings::ArgRequiredElseHelp)
        .arg(
            Arg::with_name("config-file")
                .long("config")
                .short("c")
                .value_name("CONFIG")
                .help("Path to configuration file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("tui")
                .long("tui")
                .help("Enable TUI monitor")
                .takes_value(false),
        )
        .subcommand(SubCommand::with_name("spot").about("Run Spot Trade Engine"))
        .subcommand(SubCommand::with_name("futures").about("Run Futures Trade Engine"))
        .get_matches();

    let config_file_path = args.value_of("config-file").unwrap_or(DEFAULT_CONFIG_FILE);
    let config = config::read_config(config_file_path.to_owned());

    match args.subcommand() {
        ("spot", Some(_)) => engines::spot::run(config, args.is_present("tui")),
        ("futures", Some(_)) => engines::futures::run(config, args.is_present("tui")),
        _ => {}
    }
}
