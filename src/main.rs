// use accounts;
use accounts::{
    bank_txs::rec_rules::GenEntry, chart_of_accounts::ChartOfAccounts, entry::journal::JournalLine,
    *,
};
use anyhow::Result;
use bank_txs::BankTxs;
use clap::{Arg, Command};
use entry::{
    Entry,
    journal::{JournalAmount, JournalEntry},
    raw,
};
use futures::{future, stream::TryStreamExt};
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
        .subcommand(
            Command::new("journal")
                .about("Shows journal")
                .arg(
                    Arg::new("account")
                        .short('a')
                        .long("account")
                        .help("Account filter")
                        .value_name("ACCOUNT")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("party")
                        .short('p')
                        .long("party")
                        .help("Party filter")
                        .value_name("PARTY")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("with-party")
                        .short('w')
                        .long("with-party")
                        .help("Show lines with party field"),
                ),
        )
        .subcommand(
            Command::new("balances")
                .about("Shows account balances")
                .arg(
                    Arg::new("account")
                        .short('a')
                        .long("account")
                        .help("Account filter")
                        .value_name("ACCOUNT")
                        .takes_value(true),
                )
                .arg(
                    Arg::new("party")
                        .short('p')
                        .long("party")
                        .help("Party filter")
                        .value_name("PARTY")
                        .takes_value(true),
                ),
        )
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

    if let Some(entries_arg) = matches.value_of("entries") {
        let ledger = if entries_arg == "-" {
            Ledger::new(None)
        } else {
            Ledger::new(Some(entries_arg))
        };
        if let Some(journal) = matches.subcommand_matches("journal") {
            // TODO walk dir sorted and add check to assert date order and process this iteratively instead of collecting
            // TODO solve the problem of emitting recurring entries in order
            let account = journal.value_of("account");
            let party = journal.value_of("party");
            let with_party = journal.is_present("with-party");

            let mut entries: Vec<JournalEntry> = ledger
                .journal_filtered(account, party)
                .try_collect()
                .await?;
            entries.sort_by_key(|x| x.date());
            entries.into_iter().try_for_each(|entry| {
                let rows = entry.to_row_strings(with_party)?.join("\n");
                println!("{rows}");
                anyhow::Ok(())
            })?;
        } else if let Some(balances) = matches.subcommand_matches("balances") {
            let party = balances.value_of("party");
            let account = balances.value_of("account");

            let balances = ledger.balances_filtered(account, party).await?;
            let total = balances
                .iter()
                .fold(JournalAmount::default(), |mut acc, amount| {
                    acc += *amount.1;
                    acc
                });
            let acc_pad = 32;
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
                println!("{report}")
            }
        } else if matches.subcommand_matches("payable").is_some() {
            let payables = ledger.payable().await?;
            let mut payables: Vec<_> = payables.iter().collect();
            payables.sort_by_key(|x| x.0);
            payables.iter().for_each(|(account, amount)| {
                println!("{:32} | {}", account, amount.to_row_string(12));
            });
        } else if matches.subcommand_matches("receivable").is_some() {
            let receivables = ledger.receivable().await?;
            let mut receivables: Vec<_> = receivables.iter().collect();
            receivables.sort_by_key(|x| x.0);
            receivables.iter().for_each(|(account, amount)| {
                println!("{:32} | {}", account, amount.to_row_string(12));
            });
        } else if let Some(reconcile) = matches.subcommand_matches("reconcile") {
            let account = reconcile.value_of("account").unwrap(); // required
            let txs_file = reconcile.value_of("bank txs").unwrap(); // required
            let rules_file = reconcile.value_of("rules");
            let mut txs = BankTxs::from_files(txs_file, rules_file).await?;

            ledger
                .entries_filtered(Some(account), None)
                .try_for_each(|entry: Entry| {
                    let matched = txs.match_and_rm(entry.clone());
                    if !matched.is_empty() {
                        // println!("Matched:\n{matched:?}\nwith:\n{entry:?}\n---");
                    } else {
                        eprintln!("ERROR: No matching tx for:\n{entry:?}\n---");
                    };

                    future::ready(Ok(()))
                })
                .await?;

            // TODO always reverse? or have importer handle that?
            txs.txs.iter().rev().for_each(|tx| {
                let raw_entry = txs
                    .rules
                    .apply(tx)
                    .and_then(|g| g.generate_raw_entry())
                    // map error to string for cloning
                    .map_err(|e| e.to_string());

                // possibly invalid/partial entry string
                let entry_string = raw_entry
                    .clone()
                    .map_err(anyhow::Error::msg)
                    .and_then(|re| serde_yaml::to_string(&re).map_err(anyhow::Error::new));

                // convert to full Entry in order to validate
                let entry: Result<Entry> = raw_entry
                    .map_err(anyhow::Error::msg)
                    .and_then(|re| re.try_into());

                let tx_row = tx.to_row_string();

                match (entry, entry_string) {
                    (Ok(_), Ok(mut entry_string)) => {
                        // insert comment after first `---` line from entry
                        entry_string.insert_str(4, &format!("# Entry generated from: {tx_row}\n"));
                        print!("{entry_string}")
                    }
                    (Err(entry_err), Ok(mut entry_string)) => {
                        // insert comment after first `---` line from entry
                        entry_string.insert_str(
                            4,
                            &format!("# ERROR generating entry for: {tx_row}\n# {entry_err}\n"),
                        );
                        println!("{entry_string}# ...")
                    }
                    (Err(err), Err(_)) => {
                        println!("# ERROR generating entry for: {tx_row}\n{err}")
                    }
                    (Ok(_), Err(err)) => {
                        println!("# ERROR generating entry for: {tx_row}\n{err}")
                    }
                }
            })
        }
    };
    Ok(())
}
