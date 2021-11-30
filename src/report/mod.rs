#![allow(clippy::new_without_default)]
mod raw;

use crate::account::{
    Account,
    Sign::{self, *},
    Tag,
    Type::{self, *},
};
use crate::journal_entry::JournalAmount;
use crate::money::Money;
use anyhow::{Context, Error, Result};
use async_std::fs;
use num_traits::Zero;
use std::{
    borrow::ToOwned,
    convert::{TryFrom, TryInto},
    fmt,
    str::FromStr,
};

#[derive(Debug, Default, Clone)]
pub struct ReportNode {
    pub header: String,
    pub types: Vec<Type>,
    pub names: Vec<String>,
    pub tags: Vec<Tag>,
    pub children: Vec<ReportNode>,
    /// Total for all accounts that match this node but not children
    pub total: Total,
}

/// The names of the accounts and their total balance
#[derive(Debug, Default, Clone)]
pub struct Total(pub Vec<String>, pub JournalAmount);

impl ReportNode {
    pub async fn from_file(file: &str) -> Result<Self> {
        let doc = fs::read_to_string(file).await?;
        doc.parse()
    }

    pub fn apply_balance(
        &mut self,
        (account, balance): (&Account, &JournalAmount),
    ) -> Result<bool> {
        // if doesn't match this node return false
        if !self.matches(account) {
            return Ok(false);
        }
        // attempt to apply to any children
        let mut found = false;
        for node in &mut self.children {
            if node.apply_balance((account, balance))? {
                found = true;
                break;
            }
        }
        if !found {
            // if not applied to children apply to this
            self.total.0.push(account.name.clone());
            self.total.1 += *balance;
        }
        Ok(true)
    }

    fn matches(&self, account: &Account) -> bool {
        // account type must match if specified
        // in addition to matching on name or tags if they are specified
        (self.types.is_empty()
            || self
                .types
                .iter()
                .find(|t| **t == account.acc_type)
                .is_some())
            && ((self.names.is_empty() && self.tags.is_empty())
                || (self.names.iter().find(|n| **n == account.name).is_some()
                    || self.tags.iter().find(|t| account.has_tag(t)).is_some()))
    }

    fn default_sign(&self) -> Sign {
        if self.has_type(Equity) {
            Debit
        } else if self.has_type(Revenue) {
            Credit
        } else if self.has_type(Asset) {
            Debit
        } else if self.has_type(Liability) {
            Credit
        } else {
            Debit
        }
    }

    fn has_type(&self, t1: Type) -> bool {
        self.types.iter().find(|t2| **t2 == t1).is_some()
    }

    pub fn items(&self) -> Result<Vec<(Vec<String>, Sign, Total)>> {
        Ok(self.items_with(Vec::new(), None)?.collect())
    }

    fn items_with(
        &self,
        mut path: Vec<String>,
        sign: Option<Sign>,
    ) -> Result<Box<dyn Iterator<Item = (Vec<String>, Sign, Total)>>> {
        path.push(self.header.clone());
        let sign = if self.types.is_empty() {
            sign.context("No sign for ReportNode")?
        } else {
            self.default_sign()
        };
        let mut items = Vec::new();
        items.push((path.clone(), sign, self.total()));
        let mut other = Vec::new();
        if self.total.1 != JournalAmount::default() && !self.children.is_empty() {
            let mut other_path = path.clone();
            other_path.push("Other".to_string());
            other.push((other_path, sign, self.total.clone()))
        }
        Ok(Box::new(
            items.into_iter().chain(
                self.children
                    .clone()
                    .into_iter()
                    .map(move |node| node.items_with(path.clone(), Some(sign.clone())))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .chain(other),
            ),
        ))
    }

    /// total of this node and all children
    pub fn total(&self) -> Total {
        self.children
            .iter()
            .fold(self.total.clone(), |mut grand_total, node| {
                let mut total = node.total();
                grand_total.0.append(&mut total.0);
                grand_total.1 += total.1;
                grand_total
            })
    }
}

impl TryFrom<raw::ReportNode> for ReportNode {
    type Error = Error;

    fn try_from(raw_report: raw::ReportNode) -> Result<Self> {
        let types = raw_report.types.map_or_else(
            || Ok(Vec::new()),
            |types| types.iter().map(|t| t.parse()).collect(),
        )?;
        let tags = raw_report.tags.map_or_else(
            || Ok(Vec::new()),
            |tags| tags.iter().map(|t| Tag::new(t)).collect(),
        )?;
        let names = raw_report.names.unwrap_or_else(|| Vec::new());
        let children = raw_report.breakdown.map_or_else(
            || Ok(Vec::new()),
            |raw_nodes| {
                raw_nodes
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<Vec<ReportNode>>>()
            },
        )?;
        Ok(ReportNode {
            header: raw_report.header.clone(),
            types,
            names,
            tags,
            children,
            total: Total(Vec::new(), JournalAmount::default()),
        })
    }
}

impl FromStr for ReportNode {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self, Self::Err> {
        let raw_report_node: raw::ReportNode = serde_yaml::from_str(doc)
            .with_context(|| format!("Failed to deserialize Report:\n{}", doc))?;
        let report_node: ReportNode = raw_report_node
            .try_into()
            .with_context(|| format!("Failed to convert Report"))?;
        Ok(report_node)
    }
}

impl fmt::Display for ReportNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let items = self.items().map_err(|_| std::fmt::Error::default());
        for item in items?.iter() {
            let mut indentation = (1..item.0.len()).fold(String::new(), |mut ident, _| {
                ident.push_str("  ");
                ident
            });
            let header = item
                .0
                .last()
                .map(ToOwned::to_owned)
                .unwrap_or(String::new());
            indentation.push_str(&header);
            let indented_header = indentation;
            // apply sign to journal ammount
            let total = match (item.1, item.2 .1) {
                (Credit, JournalAmount::Credit(money)) => money,
                (Credit, JournalAmount::Debit(money)) => -money,
                (Debit, JournalAmount::Debit(money)) => money,
                (Debit, JournalAmount::Credit(money)) => -money,
            };
            writeln!(f, "{:<32}{:>6}", indented_header, total)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;
    use crate::{account::Type::*, tags};

    #[test]
    fn match_tests() -> Result<()> {
        let node = ReportNode {
            types: vec![Expense],
            ..Default::default()
        };
        let account = Account {
            acc_type: Expense,
            ..Default::default()
        };
        assert!(node.matches(&account), "Matches account based on type");

        let node = ReportNode {
            tags: tags!["Current"]?,
            ..Default::default()
        };
        let account = Account {
            tags: tags!["Current", "Bank"]?,
            ..Default::default()
        };
        assert!(node.matches(&account), "Matches account based on tags");

        let node = ReportNode {
            names: vec!["Misc".to_string()],
            ..Default::default()
        };
        let account = Account {
            name: "Misc".to_string(),
            ..Default::default()
        };
        assert!(node.matches(&account), "Matches account based on name");

        let node = ReportNode {
            types: vec![Liability],
            tags: tags!["Current"]?,
            ..Default::default()
        };
        let account = Account {
            acc_type: Asset,
            tags: tags!["Current", "Bank"]?,
            ..Default::default()
        };
        assert!(
            !node.matches(&account),
            "Doesn't match if tags match but type doesn't match"
        );

        let node = ReportNode {
            types: vec![Liability],
            names: vec!["Misc".to_string()],
            ..Default::default()
        };
        let account = Account {
            name: "Misc".to_string(),
            acc_type: Asset,
            ..Default::default()
        };
        assert!(
            !node.matches(&account),
            "Doesn't match if name matches but type doesn't match"
        );

        let node = ReportNode {
            names: vec!["Rent".to_string()],
            ..Default::default()
        };
        let account = Account {
            name: "Maintenance".to_string(),
            ..Default::default()
        };
        assert!(
            !node.matches(&account),
            "Doesn't match if name doesn't match"
        );

        let node = ReportNode {
            tags: tags!["Current"]?,
            ..Default::default()
        };
        let account = Account {
            tags: tags!["Bank"]?,
            ..Default::default()
        };
        assert!(!node.matches(&account), "Doesn't match if tags don't match");

        let node = ReportNode {
            ..Default::default()
        };
        let account = Account {
            acc_type: Asset,
            tags: tags!["Current", "Bank"]?,
            ..Default::default()
        };
        assert!(node.matches(&account), "Matches if no match filters");

        let node = ReportNode {
            tags: tags!["Current"]?,
            names: vec!["Accounts Payable".to_string()],
            ..Default::default()
        };
        let account = Account {
            name: "Business Checking".to_string(),
            tags: tags!["Current", "Bank"]?,
            ..Default::default()
        };
        assert!(
            node.matches(&account),
            "Matches if tags matches even if name doesn't match"
        );

        let node = ReportNode {
            tags: tags!["Current"]?,
            names: vec!["Accounts Payable".to_string()],
            ..Default::default()
        };
        let account = Account {
            name: "Accounts Payable".to_string(),
            ..Default::default()
        };
        assert!(
            node.matches(&account),
            "Matches if name matches even if tags don't match"
        );

        Ok(())
    }
}
