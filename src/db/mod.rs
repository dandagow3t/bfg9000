mod pump_fun;
mod raydium;

use rusqlite::Connection;
use rusqlite::Result;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;

pub use pump_fun::PumpFunCoinAccounts;
pub use raydium::RaydiumCoinAccounts;

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

// Add the new trait
pub(crate) trait CoinAccounts {
    type Account;
    const TABLE_NAME: &'static str;

    fn init_table(conn: &MutexGuard<'_, Connection>) -> Result<()>;
    fn add_coin_accounts(conn: &MutexGuard<'_, Connection>, accounts: &Self::Account)
        -> Result<()>;
    fn get_coin_accounts_by_mint_address(
        conn: &MutexGuard<'_, Connection>,
        mint_address: &str,
    ) -> Result<Option<Self::Account>>;

    fn get_coin_accounts_by_coin_name(
        conn: &MutexGuard<'_, Connection>,
        mint_address: &str,
    ) -> Result<Option<Self::Account>>;
}

impl Database {
    pub async fn new(path: &Path) -> Result<Self> {
        let conn = Arc::new(Mutex::new(Connection::open(path)?));

        // Initialize both tables
        RaydiumCoinAccounts::init_table(&conn.lock().await)?;
        PumpFunCoinAccounts::init_table(&conn.lock().await)?;

        Ok(Database { conn })
    }

    // Delegate to specific modules
    pub async fn add_raydium_coin_accounts(&self, accounts: &RaydiumCoinAccounts) -> Result<()> {
        RaydiumCoinAccounts::add_coin_accounts(&self.conn.lock().await, accounts)
    }

    pub async fn get_raydium_coin_accounts(
        &self,
        mint_address: &str,
    ) -> Result<Option<RaydiumCoinAccounts>> {
        RaydiumCoinAccounts::get_coin_accounts_by_mint_address(
            &self.conn.lock().await,
            mint_address,
        )
    }

    pub async fn add_pump_fun_coin_accounts(&self, accounts: &PumpFunCoinAccounts) -> Result<()> {
        PumpFunCoinAccounts::add_coin_accounts(&self.conn.lock().await, accounts)
    }

    pub async fn get_pump_fun_coin_accounts_by_mint_address(
        &self,
        mint_address: &str,
    ) -> Result<Option<PumpFunCoinAccounts>> {
        PumpFunCoinAccounts::get_coin_accounts_by_mint_address(
            &self.conn.lock().await,
            mint_address,
        )
    }

    pub async fn get_pump_fun_coin_accounts_by_name(
        &self,
        coin_name: &str,
    ) -> Result<Option<PumpFunCoinAccounts>> {
        PumpFunCoinAccounts::get_coin_accounts_by_coin_name(&self.conn.lock().await, coin_name)
    }
}
