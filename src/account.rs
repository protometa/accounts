use self::Account::*;
use self::AssetsAccount::*;
use self::ExpensesAccount::*;
use self::LiabilitiesAccount::*;

use std::fmt;

type Name = String;

#[derive(Debug)]
pub struct Info(Name);

#[derive(Debug)]
pub struct COSInfo(Name);

#[derive(Debug)]
pub struct Party(Name);

#[derive(Debug, Clone)]
pub enum Account {
    Expenses(ExpensesAccount),
    Assets(AssetsAccount),
    Liabilities(LiabilitiesAccount),
    Equity(GenericCreditAccount),
    Revenue(GenericCreditAccount),
}

impl Account {
    pub fn new_expense_account(name: &str) -> Self {
        Expenses(ExpensesAccount::Generic(GenericDebitAccount {
            name: name.to_owned(),
        }))
    }

    pub fn new_revenue_account(name: &str) -> Self {
        Revenue(GenericCreditAccount {
            name: name.to_owned(),
        })
    }

    pub fn new_accounts_payable(party: &str) -> Self {
        Liabilities(AccountsPayable(AccountsPayableAccount {
            party: party.to_owned(),
        }))
    }

    pub fn new_accounts_receivable(party: &str) -> Self {
        Assets(AccountsReceivable(AccountsReceivableAccount {
            party: party.to_owned(),
        }))
    }

    pub fn new_bank_account(name: &str, account_number: &str) -> Self {
        Assets(Bank(BankAccount {
            name: name.to_owned(),
            account_number: account_number.to_owned(),
        }))
    }

    pub fn new_credit_card_account(name: &str, account_number: &str) -> Self {
        Liabilities(CreditCard(CreditCardAccount {
            name: name.to_owned(),
            account_number: account_number.to_owned(),
        }))
    }
}

impl fmt::Display for Account {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let string = match self {
            Expenses(ExpensesAccount::Generic(GenericDebitAccount { name }))
            | Liabilities(LiabilitiesAccount::Generic(GenericCreditAccount { name, .. }))
            | Revenue(GenericCreditAccount { name })
            | Assets(Bank(BankAccount { name, .. }))
            | Liabilities(CreditCard(CreditCardAccount { name, .. }))
            | Assets(AssetsAccount::Generic(GenericDebitAccount { name }))
            | Equity(GenericCreditAccount { name }) => name,
            Liabilities(AccountsPayable(_)) => "Accounts Payable",
            Assets(AccountsReceivable(_)) => "Accounts Receivable",
            Expenses(CostOfSales(_)) => "Cost of Sales",
        };
        write!(f, "{}", string)
    }
}

#[derive(Debug, Clone)]
pub enum ExpensesAccount {
    Generic(GenericDebitAccount),
    CostOfSales(CostOfSalesAccount),
}

#[derive(Debug, Clone)]
pub enum AssetsAccount {
    Generic(GenericDebitAccount),
    AccountsReceivable(AccountsReceivableAccount),
    Bank(BankAccount),
}

#[derive(Debug, Clone)]
pub enum LiabilitiesAccount {
    Generic(GenericCreditAccount),
    AccountsPayable(AccountsPayableAccount),
    CreditCard(CreditCardAccount),
}

#[derive(Debug, Clone)]
pub struct GenericDebitAccount {
    name: String,
}

impl GenericDebitAccount {
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[derive(Debug, Clone)]
pub struct GenericCreditAccount {
    name: String,
}

impl GenericCreditAccount {
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[derive(Debug, Clone)]
pub struct CostOfSalesAccount {
    name: String,
    code: String,
}

#[derive(Debug, Clone)]
pub struct AccountsReceivableAccount {
    party: String,
}

impl AccountsReceivableAccount {
    pub fn party(&self) -> String {
        self.party.clone()
    }
}

#[derive(Debug, Clone)]
pub struct AccountsPayableAccount {
    party: String,
}

impl AccountsPayableAccount {
    pub fn party(&self) -> String {
        self.party.clone()
    }
}

#[derive(Debug, Clone)]
pub struct BankAccount {
    name: String,
    account_number: String,
}

impl BankAccount {
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[derive(Debug, Clone)]
pub struct CreditCardAccount {
    name: String,
    account_number: String,
}

impl CreditCardAccount {
    pub fn name(&self) -> String {
        self.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expenses() -> () {
        let acc = Expenses(ExpensesAccount::Generic(GenericDebitAccount {
            name: String::from("Shop Rent"),
        }));
        dbg!(&acc);
        let is_expenses = match acc {
            Expenses(_) => true,
            _ => false,
        };
        assert_eq!(is_expenses, true);
        let name = match acc {
            Expenses(ExpensesAccount::Generic(GenericDebitAccount { name })) => name,
            _ => String::from("Other"),
        };
        assert_eq!(name, String::from("Shop Rent"));
    }

    #[test]
    fn accounts_recievable() -> () {
        let acc = Assets(AccountsReceivable(AccountsReceivableAccount {
            party: String::from("ACME Business Services"),
        }));
        dbg!(&acc);
        let party = match acc {
            Assets(AccountsReceivable(AccountsReceivableAccount { party })) => party,
            _ => String::from("Other"),
        };
        assert_eq!(party, String::from("ACME Business Services"));
    }
}
