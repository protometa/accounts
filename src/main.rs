// use accounts;
use accounts::{chart_of_accounts::ChartOfAccounts, *};
use anyhow::Result;
use bank_txs::BankTxs;
use clap::{Arg, Command};
use entry::{
    journal::{JournalAmount, JournalEntry},
    raw, Entry,
};
use futures::{future, stream::TryStreamExt};
use money::Money;
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
                    Arg::new("account")
                        .short('a')
                        .long("account")
                        .help("Bank account from ledger to reconcile")
                        .value_name("ACCOUNT")
                        .takes_value(true)
                        .required(true),
                )
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
                        .help("Rules spec file for matching and generating txs")
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
                print!("{}", entry);
            });
        } else if matches.subcommand_matches("balances").is_some() {
            let balances = ledger.balances().await?;
            let total = balances
                .iter()
                .fold(JournalAmount::default(), |mut acc, amount| {
                    acc += *amount.1;
                    acc
                });
            let acc_pad = 25;
            let amt_pad = 12;
            balances.iter().for_each(|(account, amount)| {
                let amt_string = amount.to_row_string(amt_pad);
                println!("{account:acc_pad$} | {amt_string}");
            });
            // if accounts do not balance, show difference as error
            if total != JournalAmount::default() {
                let total_string = total.to_row_string(amt_pad);
                println!("{:acc_pad$} | {total_string}", "ERROR");
            }
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
            let account = reconcile.value_of("account").unwrap(); // required
            let txs_file = reconcile.value_of("bank txs").unwrap(); // required
            let rules_file = reconcile.value_of("rules");
            let mut txs = BankTxs::from_files(txs_file, rules_file).await?;

            ledger
                .entries()
                .try_filter(|entry| {
                    let has_account = entry.amount_of_account(account).is_some();
                    future::ready(has_account)
                })
                .try_for_each(|entry: Entry| {
                    let matched = txs.match_and_rm(entry.clone());
                    if !matched.is_empty() {
                        println!("Matched:\n{matched:?}\nwith:\n{entry:?}\n---");
                    } else {
                        eprintln!("ERROR: No matching tx for:\n{entry:?}\n---");
                    };

                    future::ready(Ok(()))
                })
                .await?;

            txs.txs.iter().rev().for_each(|tx| {
                let entry = (|| {
                    let raw: raw::Entry = txs.rules.apply(tx)?.generate()?.into();
                    let entry = serde_yaml::to_string(&raw)?;
                    anyhow::Ok(entry)
                })();

                match entry {
                    Ok(entry) => println!("# Entry generated from: {tx:?}:\n{entry}---"),
                    Err(err) => eprintln!("ERROR generating:\n{tx:?}:\n{err}\n---"),
                }
            })
        }
    };
    Ok(())
}
