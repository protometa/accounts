use super::BankTx;
use crate::entry::{journal::JournalEntry, raw, Entry};
use anyhow::{anyhow, Context, Error, Ok, Result};
use rule::{arg::Arg, json, Rule};
use serde::{Deserialize, Serialize};
// use serde_yaml::Value;
use crate::entry::journal::JournalAmount::{Credit, Debit};
use serde_json::{Map, Number, Value};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    fmt::Debug,
    str::FromStr,
};

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct RawRecRule {
    rule: Value,
    values: Option<HashMap<String, Value>>,
    entry: Option<Map<String, Value>>,
}

/// Defines a reconciliation rule for bank txs
#[derive(Debug)]
pub struct RecRule {
    rule: Rule,
    values: HashMap<String, Arg>,
    template: Map<String, Value>,
}

impl TryFrom<RawRecRule> for RecRule {
    type Error = Error;

    fn try_from(raw: RawRecRule) -> Result<Self> {
        let rule = Self {
            // the error types from this crate don't play well with anyhow
            rule: Rule::new(raw.rule.clone())
                .or(Err(anyhow!("Failed to parse Rule: {:?}", raw.rule)))?,
            values: raw
                .values
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| {
                    let v = match v.clone() {
                        // if string make var expression of string
                        Value::String(string) => Arg::from_json(json!(string)),
                        _ => Arg::from_json(v.clone()),
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

impl FromStr for RecRule {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self> {
        let raw: RawRecRule = serde_yaml::from_str(doc)
            .with_context(|| anyhow!("Failed to deserialize raw Rule:\n{doc:?}"))?;

        let rule: Self = raw
            .clone()
            .try_into()
            .with_context(|| anyhow!("Failed to convert raw Rule:\n{raw:?}"))?;
        Ok(rule)
    }
}

impl RecRule {
    fn apply(&self, ge: &mut GenEntry) -> Result<()> {
        // TODO handle list of matching rules
        // TODO consider also matching on previously generated values?
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

/// List of reconciliation rules for bank txs
#[derive(Debug, Default)]
pub struct RecRules(Vec<RecRule>);

impl RecRules {
    pub fn apply(&self, tx: &BankTx) -> Result<GenEntry> {
        // create new GenEntry from tx
        let mut ge = GenEntry::new(tx);
        // iteratively apply GenEntry to each Rule
        for rule in self.0.iter() {
            // matching and updating values
            rule.apply(&mut ge)?;
            // if template rule is encountered return early
            // TODO templates can be partial so could collect fields on multiple passthrough rules instead of returning early - would just need to decide how to specify non-passthrough rules
            if !rule.template.is_empty() {
                return Ok(ge);
            }
        }
        // if all rules applied return
        Ok(ge)
    }
}

impl FromStr for RecRules {
    type Err = Error;

    fn from_str(doc: &str) -> Result<Self> {
        Ok(Self(
            doc.split("---")
                .map(|r| r.parse())
                .collect::<Result<Vec<RecRule>>>()?,
        ))
    }
}

pub struct GenEntry {
    tx: BankTx,
    values: HashMap<String, Arg>,
    template: Map<String, Value>,
}

pub fn template_deep<F>(val: &mut Value, f: F)
where
    F: Fn(&str) -> String + Clone,
{
    if let Some(map) = val.as_object_mut() {
        map.values_mut().for_each(|v| template_deep(v, f.clone()));
    }
    if let Some(arr) = val.as_array_mut() {
        arr.iter_mut().for_each(|v| template_deep(v, f.clone()));
    } else if let Some(s) = val.as_str() {
        *val = Value::String(f(s));
    }
}

#[test]
fn template_deep_test() {
    let mut obj = json!({
        "string": "test string",
        "template": "test {value}",
        "nested": {
            "template": "test {value}",
            "templates": [
                "test {value}"
            ]
        },
        "number": 12,
        "null": null
    })
    .clone();

    template_deep(&mut obj, |s| s.replace("{value}", "result"));

    assert_eq!(
        obj,
        json!({
            "string": "test string",
            "template": "test result",
            "nested": {
                "template": "test result",
                "templates": [
                    "test result"
                ]
            },
            "number": 12,
            "null": null
        })
    )
}

impl GenEntry {
    pub fn new(tx: &BankTx) -> Self {
        Self {
            tx: tx.to_owned(),
            values: HashMap::default(),
            template: Map::default(),
        }
    }

    pub fn generate(&self) -> Result<Entry> {
        let (evaled, mut templated) = self.evaluate()?;
        let templated = templated
            .as_object_mut()
            .context("template not an object")?;

        // set id if not set
        if templated.get("id").is_none() {
            templated.insert(
                "id".to_string(),
                Value::String(
                    evaled
                        .get("id")
                        .cloned()
                        // TODO generate better id from tx
                        .unwrap_or_else(|| {
                            let famount: f64 = self.tx.amount.try_into().unwrap_or_default();
                            format!("{}-{}", &self.tx.date.to_string(), &famount.to_string())
                        }),
                ),
            );
        }

        // set date if not set from values or tx
        if templated.get("date").is_none() {
            templated.insert(
                "date".to_string(),
                Value::String(
                    evaled
                        .get("date")
                        .cloned()
                        .unwrap_or(self.tx.date.to_string()),
                ),
            );
        }

        // set memo if not set from values or tx
        if templated.get("memo").is_none() {
            templated.insert(
                "memo".to_string(),
                Value::String(evaled.get("memo").cloned().unwrap_or(self.tx.memo.clone())),
            );
        }

        // set type if not set
        if templated.get("type").is_none() {
            // assume a payment type
            let default_type = match self.tx.amount {
                Credit(_) => "Payment Received".to_string(),
                Debit(_) => "Payment Sent".to_string(),
            };
            templated.insert("type".to_string(), Value::String(default_type));

            // set ammount of payment type from tx
            // (this cannot be overriden by values or template!)
            let famount: f64 = self.tx.amount.try_into()?;
            templated.insert(
                "amount".to_string(),
                Value::Number(Number::from_f64(famount).context("Can't convert to json Number")?),
            );

            // account from template or special bank_account value
            if templated.get("account").is_none() {
                if let Some(bank_account) = evaled.get("bank_account") {
                    templated.insert("account".to_string(), Value::String(bank_account.clone()));
                }
            }
        }

        let raw_entry: raw::Entry = serde_json::from_value(Value::Object(templated.clone()))?;
        Ok(raw_entry.try_into()?)
    }

    /// evaluates value expressions and apply to template
    fn evaluate(&self) -> Result<(HashMap<String, String>, Value)> {
        // TODO iteratively allow values in subsequent value expressions
        // by progressively adding them to the context
        let evaled = self
            .values
            .iter()
            .map(|(k, arg)| {
                let v = match arg {
                    Arg::Expr(exp) => exp
                        .matches(&self.tx)
                        .map_err(|e| dbg!(e))
                        .or(Err(anyhow!("Error evaluating expression: {arg:?}")))?
                        .to_string(),
                    _ => arg.to_string(),
                };
                Ok((k.to_owned(), v))
            })
            .collect::<Result<HashMap<String, String>>>()?;

        let mut templated = Value::Object(self.template.clone());
        template_deep(&mut templated, |s| {
            let mut temp = s.to_string();
            evaled.iter().for_each(|(k, v)| {
                let pattern = format!("{{{k}}}");
                temp = temp.replace(&pattern, v);
            });
            temp
        });

        Ok((evaled, templated))
    }

    /// Used to match entries to rules
    /// Create a GeneratingEntry by applying tx to rules
    /// then attempt to match it to JournalEntry
    pub fn match_entry(&self, entry: &JournalEntry) -> Result<bool> {
        // TODO since rules can generate any type of entry, allow matching on other types
        // eg if party of payment entry doesn't match

        let (evaled, templated) = self.evaluate()?;

        // TODO allow date range (bank tx dates may lag behind entries)
        // (matching of a very late payment could be overridden by a specific rule)
        // TODO get date from values or template to allow for expressions
        // TODO potentially just iterate over all available fields after evaluation and interpolation and return false on any mismatch - but may require serializing this given entry to match fields
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
        } else {
            // TODO currently bank_account value is required, but should also match on fully qualified account in templates
            return Ok(false);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod rec_rules_tests {
    use super::*;
    use crate::bank_txs::BankTx;
    use anyhow::Result;
    use indoc::indoc;

    #[test]
    fn evaluate_test() -> Result<()> {
        // TODO fork rules crate to allow for things like:
        // rule: [true] # matches everything
        // values:
        //   month: [index, [sub, month, 1], [Jan, Feb, Mar, Apr, May, Jun, Jul, Aug, Sept, Oct, Nov, Dec]] # use to get month abvr

        let tx: BankTx = "2025-03-06 | XX00 | 60.50 | | ACME Elec. Svc".parse()?;

        let rules: RecRules = indoc! {r#"
            rule: [=, account, "XX00"]
            values:
              bank_account: Bank Checking
            ---
            rule: [match, memo, "ACME Elec*"]
            values:
              month_year: [join, "/", [var, month], [var, year]]
            entry:
              memo: "{month_year} electric bill"
              party: ACME Electrical Services
        "#}
        .parse()?;

        let gen_entry = rules.apply(&tx)?;

        let (evaled, templated) = gen_entry.evaluate()?;

        assert_eq!(
            evaled,
            HashMap::from([
                ("bank_account".to_string(), "Bank Checking".to_string()),
                ("month_year".to_string(), "3/2025".to_string())
            ])
        );
        assert_eq!(
            templated,
            json!({
                "party": "ACME Electrical Services",
                "memo": "3/2025 electric bill"
            })
        );

        Ok(())
    }
}
