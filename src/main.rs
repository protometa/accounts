// use accounts;
use anyhow::Result;
use clap::{App, Arg};

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
        if matches.subcommand_matches("journal").is_some() {
            println!("Show journal for entries at {}", entries);
        } else if matches.subcommand_matches("balances").is_some() {
            println!("Show balances for entries at {}", entries);
        } else if let Some(report) = matches.subcommand_matches("report") {
            if let (Some(spec), Some(chart)) = (
                report.value_of("report spec"),
                report.value_of("chart of accounts"),
            ) {
                println!(
                    "Show report for entries at {} with spec {} and chart {}!",
                    entries, spec, chart
                );
            }
        }
    };
    Ok(())
}
