use super::account::*;

pub type AccountId = usize;

pub struct ChartOfAccounts(Vec<Account>);

impl ChartOfAccounts {
    pub fn new() -> Self {
        ChartOfAccounts(Vec::with_capacity(20))
    }

    pub fn get(&self, index: AccountId) -> Option<Account> {
        self.0.get(index).map(|account| account.to_owned())
    }

    pub fn create_bank_account(&mut self, name: &str, account_number: &str) -> Account {
        let account = Account::new_bank_account(name, account_number);
        self.0.push(account.clone());
        account
    }

    pub fn create_credit_card_account(&mut self, name: &str, account_number: &str) -> Account {
        let account = Account::new_credit_card_account(name, account_number);
        self.0.push(account.clone());
        account
    }
}
