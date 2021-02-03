use anyhow::{Context, Error, Result};
use rust_decimal::prelude::*;
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug, Clone)]
pub struct Money(pub Decimal);

/// Basically this holds a Decimal that is scaled out to at least 2 dp (doesn't round).
impl TryFrom<f64> for Money {
    type Error = Error;

    fn try_from(f: f64) -> Result<Self> {
        let mut d = Decimal::from_f64(f).context(format!("Failed to convert {} to Money", f))?;
        if d.scale() < 2 {
            d.rescale(2);
        }
        Ok(Self(d))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::TryInto;

    #[test]
    fn money_from_f64() -> Result<()> {
        // less than 2 dp
        let m: Money = 1f64.try_into()?;
        assert_eq!(m.to_string(), "$1.00");
        let m: Money = 1.1.try_into()?;
        assert_eq!(m.to_string(), "$1.10");

        let m: Money = 1.11.try_into()?;
        assert_eq!(m.to_string(), "$1.11");

        // more than 2 dp
        let m: Money = 1.111.try_into()?;
        assert_eq!(m.to_string(), "$1.111");

        Ok(())
    }
}
