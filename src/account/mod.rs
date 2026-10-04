mod raw;

use self::BalanceType::*;
use self::Class::*;
use anyhow::{Context, Error, Result, anyhow, bail};
use std::{
    convert::{TryFrom, TryInto},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Class {
    Asset,
    Liability,
    #[default]
    Equity,
    Revenue,
    Expense,
}

impl FromStr for Class {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = match s {
            "Asset" => Class::Asset,
            "Liability" => Class::Liability,
            "Equity" => Class::Equity,
            "Revenue" => Class::Revenue,
            "Expense" => Class::Expense,
            _ => bail!("Invalid account class: {s}"),
        };
        Ok(t)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum BalanceType {
    Debit,
    Credit,
}

impl FromStr for BalanceType {
    type Err = Error; // TODO custom parse error?

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "debit" => Ok(BalanceType::Debit),
            "credit" => Ok(BalanceType::Credit),
            _ => Err(anyhow!("Balance type \"{s}\" not recognized")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag(String);

impl Tag {
    pub fn new(tag: &str) -> Result<Self> {
        let limit = 32;
        if tag.len() > limit {
            bail!("Tag is longer than {limit} characters: {tag}");
        }
        Ok(Self(tag.to_lowercase()))
    }
}

#[macro_export]
macro_rules! tags {
    ($($tag:expr),*) => {
        anyhow::Ok(vec![$(Tag::new($tag)?,)*])
    };
}

#[derive(Debug, Default, Clone)]
pub struct Account {
    pub class: Class,
    pub name: String,
    pub tags: Vec<Tag>,
    pub num: Option<usize>, // unique identifier, used for ordering if present
}

impl Account {
    pub fn new(class: Class, name: &str, num: Option<usize>, tags: Vec<Tag>) -> Self {
        Account {
            name: name.to_owned(),
            class,
            tags,
            num,
        }
    }

    pub fn normal_balance(&self) -> BalanceType {
        match self.class {
            Asset | Expense => Debit,
            Liability | Revenue | Equity => Credit,
        }
    }

    pub fn is_debit(&self) -> bool {
        match self.normal_balance() {
            Debit => true,
            Credit => false,
        }
    }

    pub fn is_credit(&self) -> bool {
        !self.is_debit()
    }

    pub fn is_real(&self) -> bool {
        match self.class {
            Asset | Liability | Equity => true,
            Revenue | Expense => false,
        }
    }

    pub fn has_tag(&self, tag: &Tag) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

impl TryFrom<raw::Account> for Account {
    type Error = Error;

    fn try_from(raw_account: raw::Account) -> Result<Self> {
        let class = raw_account.class.parse()?;
        let tags = raw_account.tags.map_or_else(
            || Ok(Vec::new()),
            |tags| tags.iter().map(|t| Tag::new(t)).collect(),
        )?;
        Ok(Account {
            class,
            name: raw_account.name,
            tags,
            num: raw_account.num,
        })
    }
}

impl FromStr for Account {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self, Self::Err> {
        let raw_account: raw::Account = serde_yaml::from_str(doc)
            .with_context(|| format!("Failed to deserialize Account:\n{doc}"))?;
        let name = raw_account.name.clone();
        let account: Account = raw_account
            .try_into()
            .with_context(|| format!("Failed to convert Account: {name}"))?;
        Ok(account)
    }
}
