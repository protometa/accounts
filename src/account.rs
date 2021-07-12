use self::Class::*;
use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Class {
    Asset,
    Liability,
    Expense,
    Revenue,
    Equity,
}

#[derive(Debug, Clone, PartialEq)]
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

pub trait Account {
    fn name(&self) -> String;

    fn class(&self) -> Class;

    fn is_debit(&self) -> bool {
        match self.class() {
            Asset | Expense => true,
            Liability | Revenue | Equity => false,
        }
    }

    fn is_credit(&self) -> bool {
        !self.is_debit()
    }

    fn tags(&self) -> Vec<Tag> {
        Vec::new()
    }

    fn has_tag(&self, tag: &str) -> bool {
        self.tags().iter().any(|e| e.0 == tag.to_lowercase())
    }
}

pub struct GenericAccount {
    class: Class,
    name: String,
    tags: Vec<Tag>,
}

impl GenericAccount {
    pub fn new(class: Class, name: &str, tags: Vec<Tag>) -> Self {
        GenericAccount {
            name: name.to_owned(),
            class,
            tags,
        }
    }
}

impl Account for GenericAccount {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn class(&self) -> Class {
        self.class
    }

    fn tags(&self) -> Vec<Tag> {
        self.tags.clone()
    }
}

pub struct AccountsPayable {}

impl AccountsPayable {
    pub fn new() -> Self {
        Self {}
    }
}

impl Account for AccountsPayable {
    fn name(&self) -> String {
        String::from("Accounts Payable")
    }

    fn class(&self) -> Class {
        Liability
    }
}

pub struct AccountsReceivable {}

impl AccountsReceivable {
    pub fn new() -> Self {
        Self {}
    }
}

impl Account for AccountsReceivable {
    fn name(&self) -> String {
        String::from("Accounts Receivable")
    }

    fn class(&self) -> Class {
        Asset
    }
}

pub struct BankAccount {
    name: String,
    tags: Vec<Tag>,
    acc_number: String,
}

impl BankAccount {
    pub fn new(name: &str, acc_number: &str, tags: Vec<Tag>) -> Self {
        Self {
            name: name.to_owned(),
            acc_number: acc_number.to_owned(),
            tags,
        }
    }

    pub fn acc_number(&self) -> String {
        self.acc_number.clone()
    }
}

impl Account for BankAccount {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn class(&self) -> Class {
        Asset
    }

    fn tags(&self) -> Vec<Tag> {
        self.tags.clone()
    }
}

pub struct CreditCardAccount {
    name: String,
    tags: Vec<Tag>,
    acc_number: String,
}

impl CreditCardAccount {
    pub fn new(name: &str, acc_number: &str, tags: Vec<Tag>) -> Self {
        Self {
            name: name.to_owned(),
            acc_number: acc_number.to_owned(),
            tags,
        }
    }

    pub fn acc_number(&self) -> String {
        self.acc_number.clone()
    }
}

impl Account for CreditCardAccount {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn class(&self) -> Class {
        Liability
    }

    fn tags(&self) -> Vec<Tag> {
        self.tags.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_expense_account() -> Result<()> {
        let acc = GenericAccount::new(Expense, "Shop Rent", tags!["indirect"]?);
        assert_eq!(acc.name(), String::from("Shop Rent"));
        assert_eq!(acc.class(), Expense);
        assert_eq!(acc.is_debit(), true);
        assert_eq!(acc.is_credit(), false);
        assert_eq!(acc.has_tag("Indirect"), true); // case insensative
        Ok(())
    }
}
