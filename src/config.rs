use clap::{App, Arg};

#[derive(Debug, Clone)]
pub struct Config {
    pub test: bool,
    pub pa_source_name: Option<String>,
}

/// Parse command line args. Program may exit when running this.
pub fn parse_cmd_args() -> Config {
    let matches = App::new("Multidevice Server")
        .arg(
            Arg::with_name("test")
                .short("t")
                .long("test")
                .help("Print 'test' and close the program."),
        )
        .arg(
            Arg::with_name("pa-source-name")
                .short("s")
                .long("pa-source-name")
                .help("Name of PulseAudio audio source. The program will start recording the specified audio source.")
                .takes_value(true),
        )
        .get_matches();

    let pa_source_name = matches.value_of("pa-source-name").map(|s| s.to_owned());

    Config {
        test: matches.is_present("test"),
        pa_source_name,
    }
}
