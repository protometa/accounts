// use accounts;
use accounts::{chart_of_accounts::ChartOfAccounts, *};
use anyhow::Result;
use bank_txs::{BankTxs, ReconciliationRules};
use clap::{Arg, Command};
use entry::journal::JournalEntry;
use futures::stream::TryStreamExt;
use std::fs;

#[async_std::main]
async fn main() -> Result<()> {
    let matches = Command::new("Accounts")
        .version("0.1.0")
        .author("Luke Nimtz <luke.nimtz@gmail.com>")
        .about("Simple accounting tools")
        // .license("MIT OR Apache-2.0")
        .arg(
            Arg::new("entries")
                .short('e')
                .long("entries")
                .help("Sets directory or file of entries or '-' for stdin ")
                .value_name("DIR")
                .default_value("./")
                .takes_value(true),
        )
        .arg(
            Arg::new("party")
                .short('p')
                .long("party")
                .help("Commandlies commands to entries filtered by party")
                .value_name("PARTY")
                .takes_value(true),
        )
        .subcommand(Command::new("journal").about("Shows journal"))
        .subcommand(Command::new("balances").about("Shows account balances"))
        .subcommand(
            Command::new("report")
                .about("Runs report given report spec and chart of accounts")
                .arg(
                    Arg::new("report spec")
                        .short('s')
                        .long("spec")
                        .help("The report spec file")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::new("chart of accounts")
                        .short('c')
                        .long("chart")
                        .help("The Chart of Accounts file")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(true),
                ),
        )
        .subcommand(Command::new("payable").about("Shows accounts payable balances by party"))
        .subcommand(Command::new("receivable").about("Shows accounts receivable balances by party"))
        .subcommand(
            Command::new("reconcile")
                .about("Reconcile entries with bank transactions")
                .arg(
                    Arg::new("bank txs")
                        .short('b')
                        .long("bank-txs")
                        .help("Bank transactions file in normalized pipe delimited format")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::new("rules")
                        .short('r')
                        .long("rules")
                        .help("Rules spec file for matching txs")
                        .value_name("FILE")
                        .takes_value(true)
                        .required(false),
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
            // TODO walk dir sorted and add check to assert date order and process this iteratively instead of collecting
            // TODO solve the problem of emitting recurring entries in order
            let mut journal_entries: Vec<JournalEntry> = ledger.journal().try_collect().await?;
            // if let Some(party) = matches.value_of("party") {
            //     journal_entries = journal_entries
            //         .into_iter()
            //         .filter(|entry| entry.3.clone().map_or(false, |p| p == party))
            //         .collect()
            // }
            journal_entries.sort_by_key(|x| x.date());
            journal_entries.into_iter().for_each(|entry| {
                println!("{}", entry);
            });
        // } else if matches.subcommand_matches("balances").is_some() {
        //     let balances = ledger
        //         .balances(matches.value_of("party").map(ToOwned::to_owned))
        //         .await?;
        //     let total = balances.iter().fold(
        //         journal_entry::JournalAmount::default(),
        //         |mut acc, amount| {
        //             acc += *amount.1;
        //             acc
        //         },
        //     );
        //     balances.iter().for_each(|(account, amount)| {
        //         println!("{:25} | {}", account, amount);
        //     });
        //     if total != journal_entry::JournalAmount::default() {
        //         println!("ERROR                     | {}", total);
        //     }
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
            // } else if matches.subcommand_matches("payable").is_some() {
            //     let payables = ledger.payable().await?;
            //     let mut payables: Vec<_> = payables.iter().collect();
            //     payables.sort_by_key(|x| x.0);
            //     payables.iter().for_each(|(account, amount)| {
            //         println!("{:25} | {}", account, amount);
            //     });
            // } else if matches.subcommand_matches("receivable").is_some() {
            //     let receivables = ledger.receivable().await?;
            //     let mut receivables: Vec<_> = receivables.iter().collect();
            //     receivables.sort_by_key(|x| x.0);
            //     receivables.iter().for_each(|(account, amount)| {
            //         println!("{:25} | {}", account, amount);
            //     });
        } else if let Some(reconcile) = matches.subcommand_matches("reconcile") {
            if let Some(txs) = reconcile.value_of("bank txs") {
                let txs = BankTxs::from_file(txs).await?;
                // let rules = if let Some(rules) = reconcile.value_of("rules") {
                //     ReconciliationRules::from_file(rules)
                // } else {
                //     ReconciliationRules::new()
                // };

                // ledger.reconcile(txs, ReconciliationRules())
            }
        }
    };
    Ok(())
}
