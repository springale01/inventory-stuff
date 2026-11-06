use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[arg(short, long)]
    settings_location: Option<PathBuf>,
}
