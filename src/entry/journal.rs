#![allow(clippy::new_without_default)]

use self::JournalAmount::*;
use crate::money::Money;
use anyhow::{bail, Error, Result};
use chrono::NaiveDate;
use num_traits::Zero;
use std::cmp::Ordering;
use std::convert::TryInto;
use std::fmt;
use std::ops::{AddAssign, Deref};

pub type JournalAccount = String;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum JournalAmount {
    Debit(Money),
    Credit(Money),
}

impl Default for JournalAmount {
    fn default() -> Self {
        JournalAmount::Debit(Money::default())
    }
}

impl JournalAmount {
    pub fn new() -> Self {
        Debit(Money::zero())
    }
    pub fn debit(n: f64) -> anyhow::Result<Self> {
        Ok(Debit(n.try_into()?))
    }
    pub fn credit(n: f64) -> anyhow::Result<Self> {
        Ok(Credit(n.try_into()?))
    }
    pub fn as_debit(&self) -> Option<Money> {
        match self {
            Debit(money) => Some(*money),
            Credit(_) => None,
        }
    }
    pub fn as_credit(&self) -> Option<Money> {
        match self {
            Debit(_) => None,
            Credit(money) => Some(*money),
        }
    }
    pub fn is_debit(&self) -> bool {
        match self {
            Debit(_) => true,
            Credit(_) => false,
        }
    }
    pub fn is_credit(&self) -> bool {
        match self {
            Debit(_) => false,
            Credit(_) => true,
        }
    }
    pub fn invert(&self) -> Self {
        match self {
            Debit(money) => Credit(*money),
            Credit(money) => Debit(*money),
        }
    }
    /// Absolute amount
    pub fn abs_amount(&self) -> Money {
        match self {
            Debit(money) => *money,
            Credit(money) => *money,
        }
    }

    pub fn to_row_string(&self, pad: usize) -> String {
        match self {
            Debit(money) => format!("{:>pad$} | {:>pad$}", money.to_string(), ""),
            Credit(money) => format!("{:>pad$} | {:>pad$}", "", money.to_string()),
        }
    }
}

#[test]
fn test_row_string() -> Result<()> {
    let dr = JournalAmount::debit(100.00)?;
    let cr = JournalAmount::credit(50.00)?;
    let dr_row = dr.to_row_string(10);
    let cr_row = cr.to_row_string(10);

    dbg!(&dr_row);
    dbg!(&cr_row);

    assert_eq!(dr_row, "   $100.00 |           ");
    assert_eq!(cr_row, "           |     $50.00");

    Ok(())
}

impl TryInto<f64> for JournalAmount {
    type Error = Error;

    fn try_into(self) -> Result<f64> {
        let f = self.abs_amount().0.try_into()?;
        Ok(f)
    }
}

impl AddAssign for JournalAmount {
    fn add_assign(&mut self, other: Self) {
        // treat credit amount as negative to add
        let relative_self = match self {
            Debit(money) => *money,
            Credit(money) => -*money,
        };
        let relative_other = match other {
            Debit(money) => money,
            Credit(money) => -money,
        };
        let sum = relative_self + relative_other;
        if sum >= Money::zero() {
            *self = Debit(sum)
        } else {
            *self = Credit(-sum)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct JournalLine(pub JournalAccount, pub JournalAmount);

/// Represents simple journal entry with definite date
/// Contains reference to Entry that generated it
#[derive(Debug, Clone)]
pub struct JournalEntry {
    r#ref: String,
    date: NaiveDate,
    memo: Option<String>,
    lines: JournalLines,
}

/// a valid set of journal entry lines
#[derive(Debug, Clone)]
pub struct JournalLines(Vec<JournalLine>);

impl JournalLines {
    /// Create valid set of balanced journal lines. Given lines must be balanced or an account given against which to balance them with a new line.
    /// Also sorts the lines to be debits first as is customary.
    pub fn new(
        mut lines: Vec<JournalLine>,
        balance_account: Option<JournalAccount>,
    ) -> Result<Self> {
        if lines.is_empty() {
            bail!("Journal lines cannot be empty");
        }
        let (total_debit, total_credit) = lines.iter().fold(
            (Money::zero(), Money::zero()),
            |(mut debit, mut credit), JournalLine(_, amount)| {
                match amount {
                    Debit(money) => debit += *money,
                    Credit(money) => credit += *money,
                };
                (debit, credit)
            },
        );
        if let Some(account) = balance_account {
            if total_debit > total_credit {
                lines.push(JournalLine(account, Credit(total_debit - total_credit)));
            } else if total_credit > total_debit {
                lines.push(JournalLine(account, Debit(total_credit - total_debit)));
            };
        } else if total_debit != total_credit {
            bail!("Journal credits and debits are not equal and no balance account given");
        };
        lines.sort_by(|a, b| {
            if a.1.is_debit() && b.1.is_credit() {
                Ordering::Less
            } else if a.1.is_credit() && b.1.is_debit() {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        Ok(Self(lines))
    }
}

impl Deref for JournalLines {
    type Target = Vec<JournalLine>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl IntoIterator for JournalLines {
    type Item = JournalLine;
    type IntoIter = <Vec<JournalLine> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl JournalEntry {
    /// Creates new `JournalEntry` given metadata, lines, and optional balance account.
    ///
    /// Will return Err if the lines are not balanced unless a balance account is given
    /// against which the entry will be balanced with a new line.
    pub fn new(
        r#ref: &str,
        date: &NaiveDate,
        memo: Option<&str>,
        lines: &[JournalLine],
        balance_account: Option<JournalAccount>,
    ) -> Result<Self> {
        Ok(JournalEntry {
            r#ref: r#ref.to_owned(),
            date: date.to_owned(),
            memo: memo.map(|s| s.to_owned()),
            lines: JournalLines::new(lines.to_owned(), balance_account)?,
        })
    }

    pub fn date(&self) -> NaiveDate {
        self.date
    }

    pub fn memo(&self) -> Option<String> {
        self.memo.clone()
    }

    pub fn lines(&self) -> JournalLines {
        self.lines.clone()
    }
}

impl fmt::Display for JournalEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for JournalLine(account, amount) in self.lines() {
            let date = self.date();
            let acc_pad = 25;
            let account = account.to_string();
            let amt_string = amount.to_row_string(12);
            let memo = self.memo.clone().unwrap_or_default();
            writeln!(f, "{date} | {account:acc_pad$} | {amt_string} | {memo}",)?
        }
        Ok(())
    }
}
