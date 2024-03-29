#![allow(clippy::new_without_default)]
mod raw;

use self::Sign::*;
use self::Type::*;
use anyhow::{bail, Context, Error, Result};
use std::{
    convert::{TryFrom, TryInto},
    str::FromStr,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Type {
    Asset,
    Liability,
    Expense,
    Revenue,
    Equity,
}

impl Default for Type {
    fn default() -> Self {
        Type::Equity
    }
}

impl FromStr for Type {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = match s {
            "Expense" => Type::Expense,
            "Revenue" => Type::Revenue,
            "Asset" => Type::Asset,
            "Liability" => Type::Liability,
            "Equity" => Type::Equity,
            _ => bail!("Invalid account type {}", s),
        };
        Ok(t)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Sign {
    Debit,
    Credit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag(String);

impl Tag {
    pub fn new(tag: &str) -> Result<Self> {
        let limit = 32;
        if tag.len() > limit {
            bail!("Tag is longer than {} characters: {}", limit, tag);
        }
        Ok(Self(tag.to_lowercase()))
    }
}

#[macro_export]
macro_rules! tags {
    ($($tag:expr),*) => {{
        let mut v = Vec::new();
        $(v.push(Tag::new($tag));)*
        v.into_iter().collect::<Result<Vec<_>>>()
    }};
}

#[derive(Debug, Default)]
pub struct Account {
    pub acc_type: Type,
    pub name: String,
    pub tags: Vec<Tag>,
}

impl Account {
    pub fn new(acc_type: Type, name: &str, tags: Vec<Tag>) -> Self {
        Account {
            name: name.to_owned(),
            acc_type,
            tags,
        }
    }

    pub fn sign(&self) -> Sign {
        match self.acc_type {
            Asset | Expense => Debit,
            Liability | Revenue | Equity => Credit,
        }
    }

    pub fn is_debit(&self) -> bool {
        match self.sign() {
            Debit => true,
            Credit => false,
        }
    }

    pub fn is_credit(&self) -> bool {
        !self.is_debit()
    }

    pub fn has_tag(&self, tag: &Tag) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

impl TryFrom<raw::Account> for Account {
    type Error = Error;

    fn try_from(raw_account: raw::Account) -> Result<Self> {
        let acc_type = raw_account.r#type.parse()?;
        let tags = raw_account.tags.map_or_else(
            || Ok(Vec::new()),
            |tags| tags.iter().map(|t| Tag::new(t)).collect(),
        )?;
        Ok(Account {
            acc_type,
            name: raw_account.name,
            tags,
        })
    }
}

impl FromStr for Account {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self, Self::Err> {
        let raw_account: raw::Account = serde_yaml::from_str(doc)
            .with_context(|| format!("Failed to deserialize Account:\n{}", doc))?;
        let name = raw_account.name.clone();
        let account: Account = raw_account
            .try_into()
            .with_context(|| format!("Failed to convert Account: {}", name))?;
        Ok(account)
    }
}
