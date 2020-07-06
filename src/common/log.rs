use simplelog::*;

pub fn init_log(debug: bool) {
    CombinedLogger::init(vec![TermLogger::new(
        if debug {
            LevelFilter::Debug
        } else {
            LevelFilter::Info
        },
        Config::default(),
        TerminalMode::Stderr,
    )])
    .unwrap();
}
