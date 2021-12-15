// use accounts;
use accounts::{chart_of_accounts::ChartOfAccounts, *};
use anyhow::Result;
use clap::{App, Arg};
use futures::stream::TryStreamExt;
use std::fs;

#[async_std::main]
async fn main() -> Result<()> {
    let matches = App::new("Accounts")
        .version("0.1.0")
        .author("Luke Nimtz <luke.nimtz@gmail.com>")
        .about("Simple accounting tools")
        // .license("MIT OR Apache-2.0")
        .arg(
            Arg::new("entries")
                .short('e')
                .long("entries")
                .about("Sets directory or file of entries or '-' for stdin ")
                .value_name("DIR")
                .default_value("./")
                .takes_value(true),
        )
        .subcommand(App::new("journal").about("Shows journal"))
        .subcommand(App::new("balances").about("Shows account balances"))
        .subcommand(
            App::new("report")
                .about("Run report given report spec and chart of accounts")
                .arg(
                    Arg::new("report spec")
                        .short('s')
                        .long("spec")
                        .about("The report spec file")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::new("chart of accounts")
                        .short('c')
                        .long("chart")
                        .about("The Chart of Accounts file")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .get_matches();

    if let Some(entries) = matches.value_of("entries") {
        let ledger = if entries == "-" {
            Ledger::new(None)
        } else {
            Ledger::new(Some(entries))
        };
        if matches.subcommand_matches("journal").is_some() {
            let mut journal_entries: Vec<journal_entry::JournalEntry> =
                ledger.journal().try_collect().await?;
            journal_entries.sort_by_key(|x| x.0);
            journal_entries.into_iter().for_each(|entry| {
                println!("{}", entry);
            });
        } else if matches.subcommand_matches("balances").is_some() {
            ledger
                .balances()
                .await?
                .iter()
                .for_each(|(account, amount)| {
                    println!("| {:25} | {} |", account, amount);
                });
        } else if let Some(report) = matches.subcommand_matches("report") {
            if let (Some(spec), Some(chart)) = (
                report.value_of("report spec"),
                report.value_of("chart of accounts"),
            ) {
                let chart = ChartOfAccounts::from_file(chart).await?;
                let mut report = fs::read_to_string(spec)?.parse()?;
                let report = ledger.run_report(&chart, &mut report).await?;
                println!("{}", report)
            }
        }
    };
    Ok(())
}
