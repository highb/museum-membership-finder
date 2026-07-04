mod add;
mod scrape;
mod scrapers;
mod validate;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask", about = "Tessera data-pipeline tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Validate all JSON data files
    Validate,
    /// Add or update an institution
    AddInstitution(add::AddInstitutionArgs),
    /// Add or update a membership tier
    AddMembership(add::AddMembershipArgs),
    /// Add ZIP centroids from CSV
    AddZips(add::AddZipsArgs),
    /// Scrape network directories and merge into institutions.json
    Scrape(scrape::ScrapeArgs),
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Validate => validate::run(),
        Cmd::AddInstitution(args) => add::add_institution(args),
        Cmd::AddMembership(args) => add::add_membership(args),
        Cmd::AddZips(args) => add::add_zips(args),
        Cmd::Scrape(args) => scrape::run(args),
    };
    if let Err(e) = result {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}
