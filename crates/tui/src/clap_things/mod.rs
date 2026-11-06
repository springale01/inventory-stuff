use std::path::PathBuf;

#[derive(Debug, clap::Parser)]
pub struct Args {
    #[arg(short, long)]
    pub settings_location: Option<PathBuf>,
}
