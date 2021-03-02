use self::Account::*;
use self::AssetsAccount::*;
use self::ExpensesAccount::*;
use self::LiabilitiesAccount::*;
use anyhow::Result;

type Name = String;

#[derive(Debug)]
pub struct Info(Name);

#[derive(Debug)]
pub struct COSInfo(Name);

#[derive(Debug)]
pub struct Party(Name);

#[derive(Debug)]
pub enum Account {
    Expenses(ExpensesAccount),
    Assets(AssetsAccount),
    Liabilities(LiabilitiesAccount),
    Equity(GenericCreditAccount),
    Revenue(GenericCreditAccount),
}

#[derive(Debug)]
pub enum ExpensesAccount {
    Generic(GenericDebitAccount),
    CostOfSales(CostOfSalesAccount),
}

#[derive(Debug)]
pub enum AssetsAccount {
    Generic(GenericDebitAccount),
    AccountsRecievable(AccountsRecievableAccount),
    Bank(BankAccount),
}

#[derive(Debug)]
pub enum LiabilitiesAccount {
    Generic(GenericCreditAccount),
    AccountsPayable(AccountsPayableAccount),
    CreditCard(CreditCardAccount),
}

#[derive(Debug)]
struct GenericDebitAccount {
    name: String,
}

#[derive(Debug)]
struct GenericCreditAccount {
    name: String,
}

#[derive(Debug)]
struct CostOfSalesAccount {
    name: String,
    code: String,
}

#[derive(Debug)]
struct AccountsRecievableAccount {
    party: String,
}

#[derive(Debug)]
struct AccountsPayableAccount {
    party: String,
}

#[derive(Debug)]
struct BankAccount {
    name: String,
    account_number: String,
}

#[derive(Debug)]
struct CreditCardAccount {
    name: String,
    account_number: String,
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
        let acc = Assets(AccountsRecievable(AccountsRecievableAccount {
            party: String::from("ACME Business Services"),
        }));
        dbg!(&acc);
        let party = match acc {
            Assets(AccountsRecievable(AccountsRecievableAccount { party })) => party,
            _ => String::from("Other"),
        };
        assert_eq!(party, String::from("ACME Business Services"));
    }
}
