use super::raw;
use crate::money::Money;
use anyhow::{Context, Error, Result};
use std::convert::TryFrom;

#[derive(Debug, Clone)]
pub struct Payment {
    pub party: String,
    pub account: String,
    pub amount: Money,
}

impl TryFrom<raw::Entry> for Payment {
    type Error = Error;

    fn try_from(
        raw::Entry {
            party,
            account,
            amount,
            ..
        }: raw::Entry,
    ) -> Result<Self> {
        Ok(Self {
            party: party.context("Party required for Payment Entry")?,
            account: account.context("Account required for Payment Entry")?,
            amount: amount.context("Amount required for Payment Entry")?,
        })
    }
}
