use clap::Parser;

fn main() -> std::io::Result<()> {
    let args = tui::clap_things::Args::parse();
    app::statics::set_statics(args.settings_location);

    tui::run()
}
