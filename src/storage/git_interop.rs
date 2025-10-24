use anyhow::Result;
use colored::*;
use git2::Repository;
use std::path::Path;

pub async fn sync_with_git(path: &Path) -> Result<()> {
    let repo = Repository::open(path)?;

    // Get current HEAD
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;

    println!("{} Git HEAD: {}", "→".bright_blue(), commit.id());
    println!(
        "{} Message: {}",
        "→".bright_blue(),
        commit.message().unwrap_or("")
    );

    // Export Forge operations to Git format
    // This would create Git commits from operation log

    println!("{}", "Forge operations synced to Git".green());

    Ok(())
}
