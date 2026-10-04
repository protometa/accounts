use crate::account::BalanceType::{self, Credit, Debit};
use crate::entry::journal::{JournalAmount, JournalEntry};
use crate::money::Money;
use crate::{BalanceLine, Ledger, LedgerLine};
use anyhow::Result;
use colored_text::Colorize;
use comfy_table::*;
use futures::StreamExt;
use futures::future::{self};
use futures::stream::{self, BoxStream};
use itertools::Itertools;
use std::string::ToString;

enum TableRow<T> {
    Header,
    Body(usize, T),
    InterBody, // TODO with alternating colors separating rows, probably don't need this anymore
    Footer,
}

const DATE_COL_WIDTH: u16 = 12;
const MONEY_COL_WIDTH: u16 = 14;
const TABLE_MIN_WIDTH: u16 = 80;

static TABLE_STYLE: TableStyle = TableStyle::new()
    .header_lines(ContentLineStyle::new(' ', '│', ' '))
    .content_lines(ContentLineStyle::new(' ', '│', ' '));

// light theme
// TODO allow toggle this
static FG_COLOR: &str = "000";
static BG_COLOR: &str = "fff";
static ALT_BG_COLOR: &str = "cee";
static HEADER_BG_COLOR: &str = "acc";

// // dark theme
// static FG_COLOR: &str = "bbb";
// static BG_COLOR: &str = "221";
// static ALT_BG_COLOR: &str = "332";
// static HEADER_BG_COLOR: &str = "554";

#[derive(Default)]
pub struct RenderTableOpts {
    pub width: Option<u16>,
    pub no_colors: bool,
    pub body_only: bool,
}

fn as_table_rows<'a, T: Send + 'a>(s: BoxStream<'a, T>) -> BoxStream<'a, TableRow<T>> {
    // enumerate and intersperse InterBody markers
    let s = s
        .enumerate()
        .map(|(i, item)| TableRow::Body(i, item))
        .flat_map(|r| match r {
            TableRow::Body(i, _) => {
                if i == 0 {
                    stream::once(async { r })
                        .left_stream::<BoxStream<TableRow<T>>>()
                        .left_stream()
                } else {
                    stream::iter([TableRow::InterBody, r])
                        .left_stream()
                        .right_stream()
                }
            }
            _ => stream::empty().right_stream().right_stream(),
        });

    // add Header and Footer markers
    stream::once(async { TableRow::Header })
        .chain(s)
        .chain(stream::once(async { TableRow::Footer }))
        .boxed()
}

pub trait RenderStreamTable<'a> {
    fn render(self) -> BoxStream<'a, String>;
    fn render_with(self, opts: RenderTableOpts) -> BoxStream<'a, String>;
}

pub trait RenderTable {
    fn render(self) -> String;
    fn render_with(self, opts: RenderTableOpts) -> String;
}

impl<'a> RenderStreamTable<'a> for Ledger<'a> {
    fn render(self) -> BoxStream<'a, String> {
        self.render_with(Default::default())
    }

    fn render_with(
        self,
        RenderTableOpts {
            width,
            no_colors,
            body_only,
            ..
        }: RenderTableOpts,
    ) -> BoxStream<'a, String> {
        as_table_rows(self.0)
            .filter(move |row| {
                future::ready(match row {
                    TableRow::Body(_, _) => true,
                    _ => !body_only,
                })
            })
            .filter_map(move |row| match row {
                // comfy_table is used to render tables, but since the data is incrimental and comfy_table structure is limited, a table often represents a single row
                TableRow::Header => {
                    let mut t = Table::new();
                    let drcr = match self.1 {
                        Debit => "Dr",
                        Credit => "Cr",
                    };
                    t.load_style(TABLE_STYLE).set_header([
                        "Date",
                        "Memo",
                        "Debit",
                        "Credit",
                        format!("{drcr} Balance").as_str(),
                        // "Balance",
                    ]);
                    set_ledger_cols(&mut t, width);
                    // entire string rows are colored at once since comfy_table can only color individual cell content
                    let colorized_row = if no_colors {
                        t.to_string().clear()
                    } else {
                        t.to_string().hex(FG_COLOR).on_hex(HEADER_BG_COLOR).bold()
                    };
                    future::ready(Some(colorized_row.to_string()))
                }
                TableRow::Body(i, line) => match line {
                    Ok(line) => {
                        let mut t = Table::new();
                        let row = [
                            line.date.to_string(),
                            line.memo.unwrap_or(String::default()),
                            line.amount
                                .get_debit()
                                .map(|m| m.to_string())
                                .unwrap_or_default(),
                            line.amount
                                .get_credit()
                                .map(|m| m.to_string())
                                .unwrap_or_default(),
                            line.running_total.as_balance_type(&self.1).to_string(),
                        ];
                        t.load_style(TABLE_STYLE).add_row(row);
                        set_ledger_cols(&mut t, width);
                        let colorized_row = if no_colors {
                            t.to_string().clear()
                        } else if i % 2 == 0 {
                            t.to_string().hex(FG_COLOR).on_hex(BG_COLOR)
                        } else {
                            t.to_string().hex(FG_COLOR).on_hex(ALT_BG_COLOR)
                        };
                        future::ready(Some(colorized_row.to_string()))
                    }
                    Err(e) => future::ready(Some(format!("{e:?}"))),
                },
                TableRow::Footer => future::ready(None),
                TableRow::InterBody => future::ready(None),
            })
            .boxed()
    }
}

// the width of all static table content
// momo column will be dynamic and wrap on small screens
// (we can't use comfy_table dynamic width since it can't know the data ahead of time)
const LEDGER_STATIC_WIDTH: u16 = DATE_COL_WIDTH + MONEY_COL_WIDTH * 3 + 6;
const LEDGER_TABLE_WIDTH: u16 = 120;

fn set_ledger_cols(t: &mut comfy_table::Table, width: Option<u16>) {
    if let Some(w) = width {
        t.set_width(w);
    }
    // this tells us above or tty width
    let width = t
        .width()
        .unwrap_or(LEDGER_TABLE_WIDTH)
        .clamp(TABLE_MIN_WIDTH, LEDGER_TABLE_WIDTH);
    // println!("{width}");
    let memo_col_dyn_width = width.saturating_sub(LEDGER_STATIC_WIDTH);
    t.column_mut(0)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(DATE_COL_WIDTH)));
    t.column_mut(1)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(memo_col_dyn_width)));
    t.column_mut(2)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(3)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(4)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
}

impl<'a> RenderStreamTable<'a> for BoxStream<'a, Result<JournalEntry>> {
    fn render(self) -> BoxStream<'a, String> {
        self.render_with(Default::default())
    }

    fn render_with(
        self,
        RenderTableOpts {
            width,
            no_colors,
            body_only,
            ..
        }: RenderTableOpts,
    ) -> BoxStream<'a, String> {
        as_table_rows(self)
            .filter(move |row| {
                future::ready(match row {
                    TableRow::Body(_, _) => true,
                    _ => !body_only,
                })
            })
            .filter_map(move |row| match row {
                TableRow::Header => {
                    let mut t = Table::new();
                    let header = ["Date", "Particulars", "Debits", "Credits"];
                    t.load_style(TABLE_STYLE).set_header(header);
                    set_journal_cols(&mut t, width);
                    let colorized_row = if no_colors {
                        t.to_string().clear()
                    } else {
                        t.to_string().hex(FG_COLOR).on_hex(HEADER_BG_COLOR).bold()
                    };
                    future::ready(Some(colorized_row.to_string()))
                }
                TableRow::Body(i, journal_entry) => match journal_entry {
                    Ok(journal_entry) => {
                        let mut lines_table = Table::new();
                        lines_table.load_style(TABLE_STYLE);
                        let mut lines = journal_entry.lines().into_iter();

                        let initial = lines.next();

                        let mut table_str = String::new();

                        if let Some(l) = initial {
                            lines_table.add_row([
                                journal_entry.date().to_string(),
                                l.0,
                                l.1.get_debit().map(|m| m.to_string()).unwrap_or_default(),
                                l.1.get_credit().map(|m| m.to_string()).unwrap_or_default(),
                            ]);

                            lines.for_each(|l| {
                                lines_table.add_row([
                                    "".to_string(),
                                    l.0,
                                    l.1.get_debit().map(|m| m.to_string()).unwrap_or_default(),
                                    l.1.get_credit().map(|m| m.to_string()).unwrap_or_default(),
                                ]);
                            });
                            set_journal_cols(&mut lines_table, width);

                            table_str = lines_table.to_string();

                            let mut memo_table = Table::new();
                            memo_table
                                .load_style(TABLE_STYLE)
                                .add_row(["", format!("({})", journal_entry.memo()).as_str()]);
                            set_journal_memo_cols(&mut memo_table, width);
                            table_str.push('\n');
                            table_str.push_str(memo_table.to_string().as_str());
                        } else {
                            lines_table.add_row([
                                journal_entry.date().to_string(),
                                format!("({})", journal_entry.memo()),
                            ]);
                            set_journal_memo_cols(&mut lines_table, width);
                            table_str = lines_table.to_string();
                        }

                        if !no_colors {
                            table_str = table_str
                                .split("\n")
                                .map(|row| {
                                    if i % 2 == 0 {
                                        row.hex(FG_COLOR).on_hex(BG_COLOR).to_string()
                                    } else {
                                        row.hex(FG_COLOR).on_hex(ALT_BG_COLOR).to_string()
                                    }
                                })
                                .join("\n");
                        }

                        future::ready(Some(table_str))
                    }
                    Err(e) => future::ready(Some(format!("{e:?}"))),
                },
                TableRow::InterBody => future::ready(None),
                TableRow::Footer => future::ready(None),
            })
            .boxed()
    }
}

const JOURNAL_STATIC_WIDTH: u16 = DATE_COL_WIDTH + MONEY_COL_WIDTH * 2 + 5;
const JOURNAL_TABLE_WIDTH: u16 = 105;

fn set_journal_cols(t: &mut comfy_table::Table, width: Option<u16>) {
    if let Some(w) = width {
        t.set_width(w);
    }
    // this tells us above or tty width
    let width = t
        .width()
        .unwrap_or(JOURNAL_TABLE_WIDTH)
        .clamp(TABLE_MIN_WIDTH, JOURNAL_TABLE_WIDTH);
    let memo_col_dyn_width = width.saturating_sub(JOURNAL_STATIC_WIDTH);
    t.column_mut(0)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(DATE_COL_WIDTH)));
    t.column_mut(1)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(memo_col_dyn_width)));
    t.column_mut(2)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(3)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
}

const JOURNAL_MEMO_STATIC_WIDTH: u16 = DATE_COL_WIDTH + 3;

fn set_journal_memo_cols(t: &mut comfy_table::Table, width: Option<u16>) {
    if let Some(w) = width {
        t.set_width(w);
    }
    // this tells us above or tty width
    let width = t
        .width()
        .unwrap_or(JOURNAL_TABLE_WIDTH)
        .clamp(TABLE_MIN_WIDTH, JOURNAL_TABLE_WIDTH);
    let memo_col_dyn_width = width.saturating_sub(JOURNAL_MEMO_STATIC_WIDTH);
    t.column_mut(0)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(DATE_COL_WIDTH)));
    t.column_mut(1)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(memo_col_dyn_width)));
}

impl RenderTable for Vec<BalanceLine> {
    fn render(self) -> String {
        self.render_with(Default::default())
    }

    fn render_with(
        self,
        RenderTableOpts {
            width, no_colors, ..
        }: RenderTableOpts,
    ) -> String {
        let totals = self.iter().fold(
            (Money::default(), Money::default()),
            |(mut dr, mut cr), (_, amount)| {
                dr += amount.get_debit().unwrap_or_default();
                cr += amount.get_credit().unwrap_or_default();
                (dr, cr)
            },
        );
        // Still have to build rows like in streaming in order format foreground and background properly
        let mut t = Table::new();
        let header = ["Account", "Debit", "Credit"];
        t.load_style(TABLE_STYLE).set_header(header);
        set_balance_cols(&mut t, width);
        let colorized_row = if no_colors {
            t.to_string().clear()
        } else {
            t.to_string().hex(FG_COLOR).on_hex(HEADER_BG_COLOR).bold()
        };
        let header_string = colorized_row.to_string();

        let row_strings = self
            .iter()
            .enumerate()
            .map(|(i, (account, amount))| {
                let row = [
                    account.to_owned(),
                    amount
                        .get_debit()
                        .map(|m| m.to_string())
                        .unwrap_or_default(),
                    amount
                        .get_credit()
                        .map(|m| m.to_string())
                        .unwrap_or_default(),
                ];
                let mut t = Table::new();
                t.load_style(TABLE_STYLE).add_row(row);
                set_balance_cols(&mut t, width);
                let colorized_row = if no_colors {
                    t.to_string().clear()
                } else if i % 2 == 0 {
                    t.to_string().hex(FG_COLOR).on_hex(BG_COLOR)
                } else {
                    t.to_string().hex(FG_COLOR).on_hex(ALT_BG_COLOR)
                };
                colorized_row.to_string()
            })
            .collect();

        let mut t = Table::new();
        let total_row = ["TOTAL", &totals.0.to_string(), &totals.1.to_string()];
        t.load_style(TABLE_STYLE).add_row(total_row);
        set_balance_cols(&mut t, width);
        let colorized_row = if no_colors {
            t.to_string().clear()
        } else {
            t.to_string().hex(FG_COLOR).on_hex(HEADER_BG_COLOR).bold()
        };
        let mut total_strings = vec![colorized_row.to_string()];

        if totals.0 != totals.1 {
            let mut t = Table::new();
            let mut net = JournalAmount::Debit(totals.0);
            net += JournalAmount::Credit(totals.1);
            let total_row = [
                "NET",
                &net.get_debit().map(|m| m.to_string()).unwrap_or_default(),
                &net.get_credit().map(|m| m.to_string()).unwrap_or_default(),
            ];
            t.load_style(TABLE_STYLE).add_row(total_row);
            set_balance_cols(&mut t, width);
            let colorized_row = if no_colors {
                t.to_string().clear()
            } else {
                t.to_string().hex(FG_COLOR).on_hex(HEADER_BG_COLOR).bold()
            };
            total_strings.push(colorized_row.to_string());
        }

        [vec![header_string], row_strings, total_strings]
            .concat()
            .join("\n")
    }
}

const BALANCE_STATIC_WIDTH: u16 = MONEY_COL_WIDTH * 2 + 4;
const BALANCE_TABLE_WIDTH: u16 = 90;

fn set_balance_cols(t: &mut comfy_table::Table, width: Option<u16>) {
    if let Some(w) = width {
        t.set_width(w);
    }
    // this tells us above or tty width
    let width = t
        .width()
        .unwrap_or(BALANCE_TABLE_WIDTH)
        .clamp(TABLE_MIN_WIDTH, BALANCE_TABLE_WIDTH);
    let acc_col_dyn_width = width.saturating_sub(BALANCE_STATIC_WIDTH);
    t.column_mut(0)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(acc_col_dyn_width)));
    t.column_mut(1)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(2)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
}
