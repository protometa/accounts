use crate::entry::journal::JournalEntry;
use crate::{LedgerLine, entry::journal::BalanceType};
use anyhow::Result;
use comfy_table::*;
use futures::StreamExt;
use futures::future::{self};
use futures::stream::{self, BoxStream};
use std::cmp;

enum TableRow<T> {
    Header,
    Body(usize, T),
    InterBody,
    Footer,
}

const DATE_COL_WIDTH: u16 = 12;
const MONEY_COL_WIDTH: u16 = 14;
const TABLE_MIN_WIDTH: u16 = 80;

static HEADER_STYLE: TableStyle = TableStyle::new()
    .top_border(LineStyle::new('╭', '─', '┬', '╮'))
    .header_lines(ContentLineStyle::new('│', '│', '│'))
    .header_separator(LineStyle::new('├', '─', '┼', '┤'))
    .content_lines(ContentLineStyle::new('│', '│', '│'));

static BODY_STYLE: TableStyle =
    TableStyle::new().content_lines(ContentLineStyle::new('│', '│', '│'));

#[derive(Default)]
pub struct RenderTableOpts {
    pub balance: Option<BalanceType>,
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

pub trait RenderTable<'a> {
    fn render(self, opts: RenderTableOpts) -> BoxStream<'a, String>;
}

static LEDGER_FOOTER_STYLE: TableStyle = TableStyle::new()
    .content_lines(ContentLineStyle::new('│', ' ', '│'))
    .bottom_border(LineStyle::new('╰', '─', '┴', '╯'));

impl<'a> RenderTable<'a> for BoxStream<'a, Result<LedgerLine>> {
    fn render(
        self,
        RenderTableOpts {
            balance,
            width,
            no_colors,
            body_only,
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
                    let drcr = match balance {
                        Some(BalanceType::Debit) => "Dr",
                        Some(BalanceType::Credit) => "Cr",
                        None => "??",
                    };
                    let balance_header = format!("{drcr} Balance");
                    let mut header = vec!["Date", "Memo", "Debit", "Credit"];
                    header.push(&balance_header);
                    let header = header.iter().map(|h| {
                        let cell = Cell::new(h);
                        if no_colors {
                            cell
                        } else {
                            cell.add_attribute(Attribute::Bold)
                        }
                    });
                    t.load_style(HEADER_STYLE).set_header(header);
                    set_ledger_cols(&mut t, width);
                    future::ready(Some(t.to_string()))
                }
                TableRow::Body(i, line) => match line {
                    Ok(line) => {
                        let mut t = Table::new();
                        let row = [
                            line.date.to_string(),
                            line.memo.unwrap_or(String::default()),
                            line.amount
                                .as_abs_debit()
                                .map(|m| m.to_string())
                                .unwrap_or(String::default()),
                            line.amount
                                .as_abs_credit()
                                .map(|m| m.to_string())
                                .unwrap_or(String::default()),
                            line.running_total
                                .as_balance_type(balance.as_ref().unwrap_or(&BalanceType::Debit))
                                .to_string(),
                        ];
                        let row = row.iter().map(|h| {
                            let cell = Cell::new(h);
                            if !no_colors && i % 2 == 1 {
                                // TODO alternating background colors might improve readability but not sure how to do that without full color themes
                                cell.add_attribute(Attribute::Dim)
                            } else {
                                cell
                            }
                        });
                        t.load_style(BODY_STYLE).add_row(row);
                        set_ledger_cols(&mut t, width);
                        future::ready(Some(t.to_string()))
                    }
                    _ => future::ready(Some("ERROR".to_string())),
                },
                TableRow::Footer => {
                    let mut t = Table::new();
                    t.load_style(LEDGER_FOOTER_STYLE)
                        .add_row((0..5).map(|_| ""));
                    set_ledger_cols(&mut t, width);
                    // rm empty row, keep only bottom border
                    future::ready(Some(t.to_string().split_once('\n').unwrap().1.to_string()))
                }
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

static JOURNAL_MEMO_STYLE: TableStyle =
    TableStyle::new().content_lines(ContentLineStyle::new('│', ' ', '│'));

static JOURNAL_FOOTER_STYLE: TableStyle = TableStyle::new()
    .content_lines(ContentLineStyle::new('│', ' ', '│'))
    .bottom_border(LineStyle::new('╰', '─', '─', '╯'));

impl<'a> RenderTable<'a> for BoxStream<'a, Result<JournalEntry>> {
    fn render(
        self,
        RenderTableOpts {
            balance: _,
            width,
            no_colors,
            body_only,
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
                    let header = ["Date", "Particulars", "Debits", "Credits"]
                        .iter()
                        .map(|h| {
                            let cell = Cell::new(h);
                            if no_colors {
                                cell
                            } else {
                                cell.add_attribute(Attribute::Bold)
                            }
                        });
                    t.load_style(HEADER_STYLE).set_header(header);
                    set_journal_cols(&mut t, width);
                    future::ready(Some(t.to_string()))
                }
                TableRow::Body(_, journal_entry) => match journal_entry {
                    Ok(journal_entry) => {
                        let mut lines_table = Table::new();
                        lines_table.load_style(BODY_STYLE);
                        let mut lines = journal_entry.lines().into_iter();

                        let initial = lines.next();

                        if let Some(l) = initial {
                            lines_table.add_row([
                                journal_entry.date().to_string(),
                                l.0,
                                l.1.as_abs_debit()
                                    .map(|m| m.to_string())
                                    .unwrap_or_default(),
                                l.1.as_abs_credit()
                                    .map(|m| m.to_string())
                                    .unwrap_or_default(),
                            ]);
                        } else {
                            lines_table.add_row([journal_entry.date().to_string(), "".to_string()]);
                        }

                        lines.for_each(|l| {
                            lines_table.add_row([
                                "".to_string(),
                                l.0,
                                l.1.as_abs_debit()
                                    .map(|m| m.to_string())
                                    .unwrap_or_default(),
                                l.1.as_abs_credit()
                                    .map(|m| m.to_string())
                                    .unwrap_or_default(),
                            ]);
                        });
                        set_journal_cols(&mut lines_table, width);

                        let mut memo_table = Table::new();

                        if journal_entry.memo().is_some() {
                            let mut memo_cell = Cell::new(format!(
                                "({})",
                                journal_entry.memo().unwrap_or(String::default())
                            ));
                            if !no_colors {
                                memo_cell = memo_cell.add_attribute(Attribute::Dim);
                            }

                            memo_table.add_row([Cell::new(""), memo_cell]);
                            memo_table.load_style(JOURNAL_MEMO_STYLE);
                            set_journal_memo_cols(&mut memo_table, width);

                            future::ready(Some(
                                [lines_table.to_string(), memo_table.to_string()].join("\n"),
                            ))
                        } else {
                            future::ready(Some(lines_table.to_string()))
                        }
                    }
                    _ => future::ready(Some("ERROR".to_string())),
                },
                TableRow::InterBody => {
                    let mut t = Table::new();
                    t.load_style(JOURNAL_MEMO_STYLE);
                    t.add_row(["", ""]);
                    set_journal_memo_cols(&mut t, width);
                    future::ready(Some(t.to_string()))
                }
                TableRow::Footer => {
                    let mut t = Table::new();
                    t.load_style(JOURNAL_FOOTER_STYLE)
                        .add_row((0..2).map(|_| ""));
                    set_journal_memo_cols(&mut t, width);
                    // rm empty row, keep only bottom border
                    future::ready(Some(t.to_string().split_once('\n').unwrap().1.to_string()))
                }
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
