use super::raw;
use crate::money::Money;
use anyhow::{Error, Result};
use std::convert::TryFrom;

#[derive(Debug, Clone)]
pub struct Payment {
    pub party: String,
    pub account: String,
    pub amount: Money,
}

impl TryFrom<raw::PaymentEntry> for Payment {
    type Error = Error;

    fn try_from(
        raw::PaymentEntry {
            party,
            account,
            amount,
            ..
        }: raw::PaymentEntry,
    ) -> Result<Self> {
        Ok(Self {
            party,
            account,
            amount,
        })
    }
}
