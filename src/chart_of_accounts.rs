use super::account::*;
use std::cell::RefCell;
use Account::*;
use AssetsAccount::*;
use ExpensesAccount::*;
use LiabilitiesAccount::*;

pub type AccountId = usize;

pub struct ChartOfAccounts(RefCell<Vec<Account>>);

impl ChartOfAccounts {
    pub fn new() -> Self {
        ChartOfAccounts(RefCell::new(Vec::with_capacity(20)))
    }

    pub fn get(&self, index: AccountId) -> Option<Account> {
        let accounts = self.0.borrow();
        accounts.get(index).map(|account| account.to_owned())
    }

    pub fn get_or_create_expense_account(&self, name: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        accounts
            .iter()
            .find(|account| match account {
                Expenses(ExpensesAccount::Generic(account)) if &account.name() == name => true,
                _ => false,
            })
            .map(Clone::clone)
            .unwrap_or_else(|| {
                let account = Account::new_expense_account(name);
                accounts.push(account.clone());
                account
            })
    }
    pub fn get_or_create_revenue_account(&self, name: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        accounts
            .iter()
            .find(|account| match account {
                Revenue(account) if &account.name() == name => true,
                _ => false,
            })
            .map(Clone::clone)
            .unwrap_or_else(|| {
                let account = Account::new_revenue_account(name);
                accounts.push(account.clone());
                account
            })
    }

    pub fn get_or_create_accounts_payable(&self, party: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        accounts
            .iter()
            .find(|account| match account {
                Liabilities(AccountsPayable(account)) if &account.party() == party => true,
                _ => false,
            })
            .map(Clone::clone)
            .unwrap_or_else(|| {
                let account = Account::new_accounts_payable(party);
                accounts.push(account.clone());
                account
            })
    }

    pub fn get_or_create_accounts_receivable(&self, party: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        accounts
            .iter()
            .find(|account| match account {
                Assets(AccountsReceivable(account)) if &account.party() == party => true,
                _ => false,
            })
            .map(Clone::clone)
            .unwrap_or_else(|| {
                let account = Account::new_accounts_payable(party);
                accounts.push(account.clone());
                account
            })
    }

    pub fn get_payment_account(&self, name: &str) -> Option<Account> {
        let accounts = self.0.borrow_mut();
        accounts
            .iter()
            .find(|account| match account {
                Assets(Bank(account)) if &account.name() == name => true,
                Liabilities(CreditCard(account)) if &account.name() == name => true,
                _ => false,
            })
            .map(Clone::clone)
    }

    pub fn create_bank_account(&self, name: &str, account_number: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        let account = Account::new_bank_account(name, account_number);
        accounts.push(account.clone());
        account
    }

    pub fn create_credit_card_account(&self, name: &str, account_number: &str) -> Account {
        let mut accounts = self.0.borrow_mut();
        let account = Account::new_credit_card_account(name, account_number);
        accounts.push(account.clone());
        account
    }
}
