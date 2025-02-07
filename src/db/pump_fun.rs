use super::CoinAccounts;
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection, Result};
use tokio::sync::MutexGuard;

#[derive(Debug)]
pub struct PumpFunCoinAccounts {
    pub mint_address: String,
    pub coin_name: String,
    pub bonding_curve: String,
    pub associated_bonding_curve: String,
    pub decimals: u32,
    pub price: f64,
}

// Implement the trait
impl CoinAccounts for PumpFunCoinAccounts {
    type Account = PumpFunCoinAccounts;
    const TABLE_NAME: &'static str = "pump_fun_coin_accounts";

    fn init_table(conn: &MutexGuard<'_, Connection>) -> Result<()> {
        conn.execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {} (
                mint_address TEXT PRIMARY KEY,
                coin_name TEXT NOT NULL,
                bonding_curve TEXT NOT NULL,
                associated_bonding_curve TEXT NOT NULL,
                decimals NUMERIC NOT NULL,
                price FLOAT NOT NULL
            )",
                Self::TABLE_NAME
            ),
            [],
        )?;
        Ok(())
    }

    fn add_coin_accounts(
        conn: &MutexGuard<'_, Connection>,
        accounts: &Self::Account,
    ) -> Result<()> {
        conn.execute(
            &format!(
                "INSERT OR REPLACE INTO {} (
                mint_address, coin_name, bonding_curve, associated_bonding_curve, decimals, price
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                Self::TABLE_NAME
            ),
            params![
                accounts.mint_address,
                accounts.coin_name,
                accounts.bonding_curve,
                accounts.associated_bonding_curve,
                accounts.decimals,
                accounts.price
            ],
        )?;
        Ok(())
    }

    fn get_coin_accounts_by_mint_address(
        conn: &MutexGuard<'_, Connection>,
        mint_address: &str,
    ) -> Result<Option<Self::Account>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM {} WHERE mint_address = ?1",
            Self::TABLE_NAME
        ))?;

        let account = stmt
            .query_row(params![mint_address], |row| {
                Ok(PumpFunCoinAccounts {
                    mint_address: row.get(0)?,
                    coin_name: row.get(1)?,
                    bonding_curve: row.get(2)?,
                    associated_bonding_curve: row.get(3)?,
                    decimals: row.get(4)?,
                    price: row.get(5)?,
                })
            })
            .optional()?;

        Ok(account)
    }

    fn get_coin_accounts_by_coin_name(
        conn: &MutexGuard<'_, Connection>,
        coin_name: &str,
    ) -> Result<Option<Self::Account>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM {} WHERE coin_name = ?1",
            Self::TABLE_NAME
        ))?;

        let account = stmt
            .query_row(params![coin_name], |row| {
                Ok(PumpFunCoinAccounts {
                    mint_address: row.get(0)?,
                    coin_name: row.get(1)?,
                    bonding_curve: row.get(2)?,
                    associated_bonding_curve: row.get(3)?,
                    decimals: row.get(4)?,
                    price: row.get(5)?,
                })
            })
            .optional()?;

        Ok(account)
    }
}
