use super::CoinAccounts;
use rusqlite::OptionalExtension;
use rusqlite::{params, Connection, Result};
use tokio::sync::MutexGuard;

#[derive(Debug)]
pub struct RaydiumCoinAccounts {
    pub mint_address: String,
    pub coin_name: String,
    pub amm_id: String,
    pub amm_open_orders: String,
    pub amm_target_orders: String,
    pub pool_coin_token_account: String,
    pub pool_pc_token_account: String,
    pub serum_market: String,
    pub serum_bids: String,
    pub serum_asks: String,
    pub serum_event_queue: String,
    pub serum_coin_vault: String,
    pub serum_pc_vault: String,
    pub serum_vault_signer: String,
}

// Implement the trait
impl CoinAccounts for RaydiumCoinAccounts {
    type Account = RaydiumCoinAccounts;
    const TABLE_NAME: &'static str = "raydium_coin_accounts";

    fn init_table(conn: &MutexGuard<'_, Connection>) -> Result<()> {
        conn.execute(
            &format!(
                "CREATE TABLE IF NOT EXISTS {} (
                mint_address TEXT PRIMARY KEY,
                coin_name TEXT NOT NULL,
                amm_id TEXT NOT NULL,
                amm_open_orders TEXT NOT NULL,
                amm_target_orders TEXT NOT NULL,
                pool_coin_token_account TEXT NOT NULL,
                pool_pc_token_account TEXT NOT NULL,
                serum_market TEXT NOT NULL,
                serum_bids TEXT NOT NULL,
                serum_asks TEXT NOT NULL,
                serum_event_queue TEXT NOT NULL,
                serum_coin_vault TEXT NOT NULL,
                serum_pc_vault TEXT NOT NULL,
                serum_vault_signer TEXT NOT NULL
            )",
                Self::TABLE_NAME,
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
                mint_address, coin_name, amm_id, amm_open_orders, amm_target_orders,
                pool_coin_token_account, pool_pc_token_account, serum_market,
                serum_bids, serum_asks, serum_event_queue, serum_coin_vault,
                serum_pc_vault, serum_vault_signer
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                Self::TABLE_NAME
            ),
            params![
                accounts.mint_address,
                accounts.coin_name,
                accounts.amm_id,
                accounts.amm_open_orders,
                accounts.amm_target_orders,
                accounts.pool_coin_token_account,
                accounts.pool_pc_token_account,
                accounts.serum_market,
                accounts.serum_bids,
                accounts.serum_asks,
                accounts.serum_event_queue,
                accounts.serum_coin_vault,
                accounts.serum_pc_vault,
                accounts.serum_vault_signer
            ],
        )?;
        Ok(())
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
                Ok(RaydiumCoinAccounts {
                    mint_address: row.get(0)?,
                    coin_name: row.get(1)?,
                    amm_id: row.get(2)?,
                    amm_open_orders: row.get(3)?,
                    amm_target_orders: row.get(4)?,
                    pool_coin_token_account: row.get(5)?,
                    pool_pc_token_account: row.get(6)?,
                    serum_market: row.get(7)?,
                    serum_bids: row.get(8)?,
                    serum_asks: row.get(9)?,
                    serum_event_queue: row.get(10)?,
                    serum_coin_vault: row.get(11)?,
                    serum_pc_vault: row.get(12)?,
                    serum_vault_signer: row.get(13)?,
                })
            })
            .optional()?;

        Ok(account)
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
                Ok(RaydiumCoinAccounts {
                    mint_address: row.get(0)?,
                    coin_name: row.get(1)?,
                    amm_id: row.get(2)?,
                    amm_open_orders: row.get(3)?,
                    amm_target_orders: row.get(4)?,
                    pool_coin_token_account: row.get(5)?,
                    pool_pc_token_account: row.get(6)?,
                    serum_market: row.get(7)?,
                    serum_bids: row.get(8)?,
                    serum_asks: row.get(9)?,
                    serum_event_queue: row.get(10)?,
                    serum_coin_vault: row.get(11)?,
                    serum_pc_vault: row.get(12)?,
                    serum_vault_signer: row.get(13)?,
                })
            })
            .optional()?;

        Ok(account)
    }
}
