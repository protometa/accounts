#![allow(clippy::new_without_default)]
use super::account::*;

pub type AccountId = usize;

pub struct ChartOfAccounts(Vec<Box<dyn Account>>);

impl ChartOfAccounts {
    pub fn new() -> Self {
        ChartOfAccounts(Vec::with_capacity(20))
    }
}
