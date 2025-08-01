use super::BankTx;
use crate::entry::{journal::JournalEntry, Entry};
use anyhow::{anyhow, Context, Error, Ok, Result};
use rule::{json, rule::Expr, Rule};
use serde::{Deserialize, Serialize};
// use serde_yaml::Value;
use serde_json::{Map, Value};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    fmt::Debug,
    str::FromStr,
};

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct RawReconciliationRule {
    rule: Value,
    values: HashMap<String, Value>,
    entry: Option<Map<String, Value>>,
}

#[derive(Debug)]
pub struct ReconciliationRule {
    rule: Rule,
    values: HashMap<String, Expr>,
    template: Map<String, Value>,
}

impl TryFrom<RawReconciliationRule> for ReconciliationRule {
    type Error = Error;

    fn try_from(raw: RawReconciliationRule) -> Result<Self> {
        let rule = Self {
            // the error types from this crate don't play well with anyhow
            rule: Rule::new(raw.rule.clone())
                .or(Err(anyhow!("Failed to parse Rule: {:?}", raw.rule)))?,
            values: raw
                .values
                .into_iter()
                .map(|(k, v)| {
                    let v = match v.clone() {
                        // if string make var expression of string
                        Value::String(string) => Expr::new(json!(["var", string])),
                        _ => Expr::new(v.clone()),
                    }
                    .or(Err(anyhow!("Failed to parse Expr: {v}")))?;
                    Ok((k, v))
                })
                .collect::<Result<_>>()?,
            // TODO handle template is not object
            template: raw.entry.unwrap_or_default(),
        };
        Ok(rule)
    }
}

impl FromStr for ReconciliationRule {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self> {
        let raw: RawReconciliationRule = serde_yaml::from_str(doc)
            .with_context(|| anyhow!("Failed to deserialize raw Rule:\n{doc:?}"))?;

        let rule: Self = raw
            .clone()
            .try_into()
            .with_context(|| anyhow!("Failed to convert raw Rule:\n{raw:?}"))?;
        Ok(rule)
    }
}

impl ReconciliationRule {
    fn apply(&self, ge: &mut GeneratingEntry) -> Result<()> {
        // TODO handle list of matching rules
        if self
            .rule
            .matches(&ge.tx)
            .or(Err(anyhow!("Error matching rule")))?
        {
            // rule matches here so merge values
            ge.values.extend(self.values.clone());
            // also merge template
            // TODO do deep merge
            ge.template.extend(self.template.clone());
        };
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct ReconciliationRules(Vec<ReconciliationRule>);

impl ReconciliationRules {
    pub fn apply(&self, tx: &BankTx) -> Result<GeneratingEntry> {
        // create new GeneratingEntry from tx
        let mut ge = GeneratingEntry::new(tx);
        // iteratively apply GeneratingEntry to each Rule
        for rule in self.0.iter() {
            // matching and updating values
            rule.apply(&mut ge)?;
            // if template rule is encountered return early
            if !rule.template.is_empty() {
                return Ok(ge);
            }
        }
        // if all rules applied return
        Ok(ge)
    }
}

impl FromStr for ReconciliationRules {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self> {
        Ok(Self(
            doc.split("---")
                .map(|r| r.parse())
                .collect::<Result<Vec<ReconciliationRule>>>()?,
        ))
    }
}

pub struct GeneratingEntry {
    tx: BankTx,
    values: HashMap<String, Expr>,
    template: Map<String, Value>,
}

impl GeneratingEntry {
    pub fn new(tx: &BankTx) -> Self {
        Self {
            tx: tx.to_owned(),
            values: HashMap::default(),
            template: Map::default(),
        }
    }

    pub fn generate(&self) -> Result<Entry> {
        todo!()
        // TODO iteratively allow values in subsequent value expressions
        // by progressively adding them to the context

        // let evaled = self
        //     .values
        //     .iter()
        //     .map(|(k, exp)| {
        //         let v = exp
        //             .matches(&ge.tx)
        //             .or(Err(anyhow!("Error evaluating expression")))?
        //             .to_string();
        //         Ok((k.to_owned(), v))
        //     })
        //     .collect::<Result<HashMap<String, String>>>()?;
    }

    fn apply_template(&self) {
        todo!()
    }

    /// evaluates value expressions to strings
    fn evaluate(&self) -> Result<HashMap<String, String>> {
        // TODO iteratively allow values in subsequent value expressions
        // by progressively adding them to the context

        self.values
            .iter()
            .map(|(k, exp)| {
                let v = exp
                    .matches(&self.tx)
                    .or(Err(anyhow!("Error evaluating expression")))?
                    .to_string();
                Ok((k.to_owned(), v))
            })
            .collect::<Result<HashMap<String, String>>>()
    }

    /// Used to match entries to rules
    /// Create a GeneratingEntry by applying tx to rules
    /// then attempt to match it to JournalEntry
    pub fn match_entry(&self, entry: &JournalEntry) -> Result<bool> {
        // TODO since rules can generate any type of entry, allow matching on other types
        // eg if party of payment entry doesn't match

        let evaled = self.evaluate()?;

        // TODO allow date range
        // (matching of a very late payment could be overridden by a specific rule)
        // TODO get date from values or template to allow for expressions
        // TODO potentially just iterate over all available fields after evaluation and interpolation and return false on any mismatch
        if self.tx.date != entry.date() {
            return Ok(false);
        };
        if let Some(bank_account) = evaled.get("bank_account") {
            if entry
                .lines()
                .iter()
                .any(|line| line.0 == *bank_account && line.1 != self.tx.amount.invert())
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
