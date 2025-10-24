use anyhow::Result;
use colored::*;
use std::path::Path;

pub async fn sync_with_git(_path: &Path) -> Result<()> {
    println!(
        "{} Git sync is currently experimental and performs a dry run only.",
        "⚠️".bright_yellow()
    );
    println!(
        "{} Please continue using native git commands alongside Forge for production.",
        "→".bright_blue()
    );

    Ok(())
}
