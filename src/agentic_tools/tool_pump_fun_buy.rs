use crate::{bot::PumpFunTxSend, db::Database};
use anyhow::Result;
use helius::Helius;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;
use solana_sdk::pubkey::Pubkey;
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;

#[derive(Deserialize, Debug)]
pub struct PumpFunBuyArgs {
    /// The mint address of the meme coin to buy
    mint: String,
    /// Maximum amount of SOL to spend (including fees)
    max_sol: f64,
    /// Maximum slippage percentage (0-100)
    slippage: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum PumpFunError {
    #[error("Invalid SOL amount: {0}")]
    InvalidSolAmount(f64),
    #[error("Invalid slippage: {0}")]
    InvalidSlippage(f64),
    #[error("Transaction error: {0}")]
    TransactionError(String),
    #[error("No accounts configured for mint address: {0}")]
    NoAccountsConfigured(String),
    #[error("Join error: {0}")]
    JoinError(tokio::task::JoinError),
}

pub struct ToolPumpFunBuy {
    helius: Arc<Mutex<Helius>>,
    signer_prv_key: Arc<String>,
    db: Arc<Database>,
}

impl ToolPumpFunBuy {
    pub fn new(helius: Arc<Mutex<Helius>>, signer_prv_key: Arc<String>, db: Arc<Database>) -> Self {
        Self {
            helius,
            signer_prv_key,
            db,
        }
    }
}

impl Tool for ToolPumpFunBuy {
    const NAME: &'static str = "pump_fun_buy";
    type Error = PumpFunError;
    type Args = PumpFunBuyArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        serde_json::from_value(json!({
            "name": "pump_fun_buy",
            "description": "Buy a meme coin on Pump.fun using SOL",
            "parameters": {
                "type": "object",
                "required": ["mint", "max_sol", "slippage"],
                "properties": {
                    "mint": {
                        "type": "string",
                        "description": "The mint address of the meme coin to buy or the meme coin name. If the meme coin is provided the mint address should be retrieved from the local cache."
                    },
                    "max_sol": {
                        "type": "number",
                        "description": "Maximum amount of SOL to spend (including fees). If the max SOL is not provided, the tool will use the default value of 0.01 SOL."
                    },
                    "slippage": {
                        "type": "number",
                        "description": "Maximum slippage percentage (0-100). If the slippage is not provided, the tool will use the default value of 10."
                    }
                }
            }
        }))
        .expect("Tool Definition")
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let helius = Arc::clone(&self.helius);
        let signer_prv_key = Arc::clone(&self.signer_prv_key);
        let db = Arc::clone(&self.db);

        // Spawing a new tokio task to comply with the trait bounds of Send + Sync
        tokio::spawn(async move {
            println!("[ToolPumpFunBuy] Args {:?}", args);
            // // Validate inputs
            // if let Err(_) = Pubkey::from_str(&args.mint) {
            //     return Err(PumpFunError::InvalidMint(args.mint));
            // }

            if args.max_sol <= 0.0 {
                return Err(PumpFunError::InvalidSolAmount(args.max_sol));
            }

            if args.slippage < 0.0 || args.slippage > 100.0 {
                return Err(PumpFunError::InvalidSlippage(args.slippage));
            }

            let accounts = match db
                .get_pump_fun_coin_accounts_by_mint_address(&args.mint)
                .await
                .map_err(|_| PumpFunError::NoAccountsConfigured(args.mint.clone()))?
            {
                Some(accounts) => accounts,
                None => match db
                    .get_pump_fun_coin_accounts_by_name(&args.mint.to_ascii_uppercase())
                    .await
                    .map_err(|_| PumpFunError::NoAccountsConfigured(args.mint.clone()))?
                {
                    Some(accounts) => accounts,
                    None => return Err(PumpFunError::NoAccountsConfigured(args.mint)),
                },
            };

            println!("[ToolPumpFunBuy] Accounts: {:?}", accounts);

            // Convert SOL to lamports (1 SOL = 1_000_000_000 lamports)
            let max_sol_lamports = (args.max_sol * 1_000_000_000.0) as u64;

            // Convert SOL price to lamports
            let price_in_sol_lamports = (accounts.price * 1_000_000_000.0) as f64;

            // Convert slippage to basis points (1% = 100 bps)
            let slippage_bps = (args.slippage * 100.0) as u64;

            let bonding_curve = Pubkey::from_str(&accounts.bonding_curve).unwrap();
            let associated_bonding_curve =
                Pubkey::from_str(&accounts.associated_bonding_curve).unwrap();
            let mint = Pubkey::from_str(&accounts.mint_address).unwrap();

            match PumpFunTxSend::buy(
                helius,
                signer_prv_key,
                bonding_curve,
                associated_bonding_curve,
                mint,
                accounts.decimals,
                price_in_sol_lamports,
                max_sol_lamports,
                slippage_bps,
            )
            .await
            {
                Ok(tx_id) => Ok(format!(
                    "Pump.fun buy transaction sent successfully, https://solscan.io/tx/{}",
                    tx_id
                )),
                Err(e) => Err(PumpFunError::TransactionError(e.to_string())),
            }
        })
        .await
        .map_err(PumpFunError::JoinError)?
    }
}
