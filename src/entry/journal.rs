#![allow(clippy::new_without_default)]

use self::JournalAmount::*;
use crate::money::Money;
use anyhow::{Error, Result};
use chrono::NaiveDate;
use num_traits::Zero;
use std::convert::TryInto;
use std::fmt;
use std::ops::AddAssign;

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
    pub fn invert(&self) -> Self {
        match self {
            Debit(money) => Credit(*money),
            Credit(money) => Debit(*money),
        }
    }
    /// Absolute amount
    pub fn abs_amount(&self) -> Money {
        match self {
            Debit(money) => money.clone(),
            Credit(money) => money.clone(),
        }
    }
}

impl TryInto<f64> for JournalAmount {
    type Error = Error;

    fn try_into(self) -> Result<f64> {
        let f = self.abs_amount().0.try_into()?;
        Ok(f)
    }
}

impl fmt::Display for JournalAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debit(debit) => write!(f, "{:>12} |             ", debit.to_string()),
            Self::Credit(credit) => write!(f, "             | {:>12}", credit.to_string()),
        }
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

impl fmt::Display for JournalLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(account, amount) = self;
        write!(f, "{:25} | {}", account.to_string(), amount)
    }
}

/// Represents simple journal entry with definite date
/// Contains reference to Entry that generated it
#[derive(Debug, Clone)]
pub struct JournalEntry {
    r#ref: String,
    date: NaiveDate,
    memo: Option<String>,
    lines: Vec<JournalLine>,
}

impl JournalEntry {
    pub fn new(r#ref: &str, date: &NaiveDate, memo: Option<&str>, lines: &[JournalLine]) -> Self {
        JournalEntry {
            r#ref: r#ref.to_owned(),
            date: date.to_owned(),
            memo: memo.map(|s| s.to_owned()),
            lines: lines.to_owned(),
        }
    }

    pub fn date(&self) -> NaiveDate {
        self.date
    }

    pub fn memo(&self) -> Option<String> {
        self.memo.clone()
    }

    pub fn lines(&self) -> Vec<JournalLine> {
        self.lines.clone()
    }
}

impl fmt::Display for JournalEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for JournalLine(account, amount) in self.lines() {
            write!(
                f,
                "{} | {:25} | {} | {} | {}",
                self.date,
                account.to_string(),
                amount,
                self.memo.clone().unwrap_or_default(),
                self.r#ref
            )?
        }
        Ok(())
    }
}
