use clap::Parser;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = tui::clap_things::Args::parse();
    app::initialize(args.settings_location).await?;

    let ui_result = tui::run().await;
    let shutdown_result = app::PineappleState::shutdown_global().await;
    ui_result?;
    shutdown_result?;
    Ok(())
}
