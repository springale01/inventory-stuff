use clap::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = tui::clap_things::Args::parse();
    app::set_statics(args.settings_location)?;

    let ui_result = tui::run();
    let shutdown_result = app::shutdown_statics();
    ui_result?;
    shutdown_result?;
    Ok(())
}
