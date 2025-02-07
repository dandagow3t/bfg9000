use std::sync::Arc;

use helius::Helius;
use serde::Serialize;
use serde_json::{from_str, Value};
use solana_program::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use spl_token::state::Account as TokenAccount;

use crate::bot::tx_common::GetSignature;
use crate::bot::AmmInfo;
use crate::constants::{
    BOT_RAYDIUM_OPERATION_BUY, BOT_RAYDIUM_OPERATION_SELL, RAYDIUM_ACCOUNTS_LEN_SWAP_BASE_IN,
    RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM, RAYDIUM_SWAP_BASE_IN_INSTRUCTION,
    RAYDIUM_SWAP_BASE_OUT_INSTRUCTION, WSOL_MINT,
};
use crate::errors::BotError;

use super::tx_common::{GetComputeData, GetTxCommon};

/// Given a transaction having among it's accounts a Raydium Liquidity Pool V4 account,
/// this module provides a method to detect if the transaction is a Pump.fun originated meme coin swap.
///
/// tx. ex. https://solscan.io/tx/oMVDZfRtPmG2pknt2dx6QS4sDpg5nj35Jn6WBmcSjj5e5rNb5K4cqSYjwxdgXdaiqtdG5fVL2qTrtjqtzSpDCZL
///
/// The following checks are performed:
///
/// 1. The Raydium swap instruction is searched for by program id = Raydium Liquidity Pool V4 program id
///
/// 2. The Raydium MEME-SOL Liquidity Pool address is extracted from position #2
/// ex. Raydium (SOL-MOOO) Market - https://solscan.io/account/9wcGEggAHCwQZCuJcrT9FJsLdkibZ3b7ZoTkknSzhkZK
///
/// 3. The metadata of the Raydium MEME-SOL Account is fetched and the Quote Mint account is extracted
/// ex. MOOO Mint account - https://solscan.io/token/D6yQGBYqxX6MRvFGqXHt6qfpX1aCJx2rPXhhWzZppump
///
/// 4. The MEME Mint account authority is checked against the known Pump.fun Mint authority
/// ex. MOOO Mint authority - https://solscan.io/account/TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM
///
/// If all checks pass, the transaction is identified as a Pump.fun originated meme coin swap and all
/// relevant information are extracted and returned.

#[derive(Debug, Serialize)]
pub struct RaydiumMemeTradeData {
    pub meme_mint: Pubkey,
    pub operation: u8,
}

/// Raydium SwapBaseIn instruction data
#[derive(Debug, Serialize)]
pub struct RaydiumSwapBaseInData {
    pub instruction: u8,
    pub amount_in: u64,
    pub minimum_amount_out: u64,
}

/// Raydium SwapBaseOut instruction data
#[derive(Debug, Serialize)]
pub struct RaydiumSwapBaseOutData {
    pub instruction: u8,
    pub amount_out: u64,
    pub max_amount_in: u64,
}

/// Raydium swap instruction data
#[derive(Debug, Serialize)]
pub enum RaydiumSwapData {
    BaseIn(RaydiumSwapBaseInData),
    BaseOut(RaydiumSwapBaseOutData),
}

/// Inner instruction data
#[derive(Debug, Serialize)]
pub struct InnerIxData {
    pub source_amount: u64,
    pub dest_amount: u64,
}

/// Raydium swap instruction accounts
#[derive(Debug, Serialize)]
pub struct RaydiumAccounts {
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
    pub user_source_token_account: String,
    pub user_destination_token_account: String,
    pub user_source_owner: String,
}

#[derive(Debug, Serialize, Default)]
pub struct RaydiumMemeTx {
    pub signature: Option<String>,
    pub accounts: Option<RaydiumAccounts>,
    pub ix_data: Option<RaydiumSwapData>,
    pub inner_ix_data: Option<InnerIxData>,
    pub meme_trade_data: Option<RaydiumMemeTradeData>,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
}

impl GetSignature for RaydiumMemeTx {}

impl GetComputeData for RaydiumMemeTx {}

impl GetTxCommon for RaydiumMemeTx {}

impl RaydiumMemeTx {
    /// Creates a new RaydiumMemeTx instance from a transaction payload.
    ///
    /// * `payload` - The transaction payload
    /// * `helius` - The Helius client
    pub async fn new(payload: String, helius: Arc<Helius>) -> Self {
        let json: Value = from_str(payload.as_str()).expect("Invalid JSON");
        let signature = match Self::get_signature(&json) {
            Some(signature) => signature,
            None => return Self::default(),
        };

        let raydium_swap_ix = match Self::get_raydium_swap_instruction(&json) {
            Some(raydium_swap_ix) => raydium_swap_ix,
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: None,
                    ix_data: None,
                    inner_ix_data: None,
                    meme_trade_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };
        let accounts = match Self::get_accounts(&raydium_swap_ix) {
            Some(accounts) => accounts,
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: None,
                    ix_data: None,
                    inner_ix_data: None,
                    meme_trade_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };
        let meme_trade_data = match Self::get_and_validate_meme_mint(helius, &accounts).await {
            Some(meme_trade_data) => meme_trade_data,
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: Some(accounts),
                    ix_data: None,
                    inner_ix_data: None,
                    meme_trade_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };

        match raydium_swap_ix["data"].as_str() {
            Some(data) => {
                match Self::get_instruction_data(data) {
                    Some(ix_data) => {
                        let (source_amount, dest_amount) = match Self::get_amounts(&json, &accounts)
                        {
                            Some((source_amount, dest_amount)) => (source_amount, dest_amount),
                            None => {
                                return Self {
                                    signature: Some(signature),
                                    accounts: Some(accounts),
                                    ix_data: Some(ix_data),
                                    inner_ix_data: None,
                                    meme_trade_data: None,
                                    compute_unit_limit: 0,
                                    compute_unit_price: 0,
                                }
                            }
                        };
                        let (compute_unit_limit, compute_unit_price) =
                            Self::get_compute_data(&json).unwrap_or((0, 0));
                        return Self {
                            signature: Some(signature),
                            accounts: Some(accounts),
                            ix_data: Some(ix_data),
                            inner_ix_data: Some(InnerIxData {
                                source_amount,
                                dest_amount,
                            }),
                            meme_trade_data: Some(meme_trade_data),
                            compute_unit_limit,
                            compute_unit_price,
                        };
                    }
                    None => {
                        return Self {
                            signature: Some(signature),
                            accounts: Some(accounts),
                            ix_data: None,
                            inner_ix_data: None,
                            meme_trade_data: None,
                            compute_unit_limit: 0,
                            compute_unit_price: 0,
                        }
                    }
                };
            }
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: Some(accounts),
                    ix_data: None,
                    inner_ix_data: None,
                    meme_trade_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        }
    }

    /// Extracts the exact amounts swapped from the inner instructions
    /// so that, using those numbers, the exact price can be computed.
    ///
    /// * `json` - The transaction JSON
    /// * `meme_accounts` - The meme accountscar
    fn get_amounts(json: &Value, meme_accounts: &RaydiumAccounts) -> Option<(u64, u64)> {
        let inner_instructions =
            match json["params"]["result"]["transaction"]["meta"]["innerInstructions"].as_array() {
                Some(inner_instructions) => inner_instructions,
                None => return None,
            };

        let default_array: Vec<Value> = Vec::new();
        // Find the relevant token transfers in a single pass
        let (source_amount, dest_amount) = inner_instructions
            .iter()
            .flat_map(|inner_ix| {
                inner_ix["instructions"]
                    .as_array()
                    .unwrap_or(&default_array)
            })
            .filter(|ix| ix["programId"].as_str() == Some(&spl_token::id().to_string()))
            .filter_map(|ix| {
                let info = &ix["parsed"]["info"];
                let amount = info["amount"].as_str()?.parse::<u64>().ok()?;

                if info["source"].as_str() == Some(&meme_accounts.user_source_token_account) {
                    Some((amount, 0))
                } else if info["destination"].as_str()
                    == Some(&meme_accounts.user_destination_token_account)
                {
                    Some((0, amount))
                } else {
                    None
                }
            })
            .fold((0, 0), |(src, dst), (s, d)| (src + s, dst + d));

        Some((source_amount, dest_amount))
    }

    /// Extracts the meme mint account from the AMM Info account data.
    /// For that it gets the Account Data using the RPC Client.
    ///
    /// It checks if the Mint is for a Pump.fun meme coin.
    /// It also detects if it's a Buy or Sell.
    ///
    /// * `helius` - The Helius client
    /// * `meme_accounts` - The meme accounts
    async fn get_and_validate_meme_mint(
        helius: Arc<Helius>,
        accounts: &RaydiumAccounts,
    ) -> Option<RaydiumMemeTradeData> {
        // Get AMM account data
        let account_data = match helius
            .async_connection()
            .unwrap()
            .get_account_data(&Pubkey::from_str_const(&accounts.amm_id))
            .await
        {
            Ok(account_data) => account_data,
            Err(_) => return None,
        };

        let amm_info = match AmmInfo::try_from_bytes(&account_data) {
            Ok(amm_info) => amm_info,
            Err(_) => return None,
        };
        let pc_vault_mint = amm_info.pc_vault_mint;

        // Only process meme tokens ending with "pump"
        if !pc_vault_mint.to_string().ends_with("pump") {
            return None;
        }

        // Default to buy operation
        let mut operation = BOT_RAYDIUM_OPERATION_BUY;

        // Check if source token is WSOL to determine operation type
        if let Ok(source_data) = helius
            .async_connection()
            .unwrap()
            .get_account_data(&Pubkey::from_str_const(&accounts.user_source_token_account))
            .await
        {
            let source_account = TokenAccount::unpack(&source_data).unwrap();
            if !source_account.mint.to_string().eq(&WSOL_MINT.to_string()) {
                operation = BOT_RAYDIUM_OPERATION_SELL;
            }
        }

        Some(RaydiumMemeTradeData {
            meme_mint: pc_vault_mint,
            operation,
        })
    }

    /// Extracts the data from the Raydium swap instruction
    ///
    /// * `input` - The instruction data
    fn get_instruction_data(input: &str) -> Option<RaydiumSwapData> {
        let input = match bs58::decode(input).into_vec() {
            Ok(input) => input,
            Err(_) => return None,
        };
        let input = input.as_slice();

        let (&tag, rest) = match input.split_first() {
            Some((tag, rest)) => (tag, rest),
            None => return None,
        };

        match tag {
            RAYDIUM_SWAP_BASE_IN_INSTRUCTION => {
                let (amount_in, rest) = match Self::unpack_u64(rest) {
                    Ok((amount_in, rest)) => (amount_in, rest),
                    Err(_) => return None,
                };
                let (minimum_amount_out, _rest) = match Self::unpack_u64(rest) {
                    Ok((minimum_amount_out, _rest)) => (minimum_amount_out, _rest),
                    Err(_) => return None,
                };

                Some(RaydiumSwapData::BaseIn(RaydiumSwapBaseInData {
                    instruction: RAYDIUM_SWAP_BASE_IN_INSTRUCTION,
                    amount_in,
                    minimum_amount_out,
                }))
            }
            RAYDIUM_SWAP_BASE_OUT_INSTRUCTION => {
                let (max_amount_in, rest) = match Self::unpack_u64(rest) {
                    Ok((max_amount_in, rest)) => (max_amount_in, rest),
                    Err(_) => return None,
                };
                let (amount_out, _rest) = match Self::unpack_u64(rest) {
                    Ok((amount_out, _rest)) => (amount_out, _rest),
                    Err(_) => return None,
                };

                Some(RaydiumSwapData::BaseOut(RaydiumSwapBaseOutData {
                    instruction: RAYDIUM_SWAP_BASE_OUT_INSTRUCTION,
                    max_amount_in,
                    amount_out,
                }))
            }
            _ => None,
        }
    }

    /// Unpacks a u64 from the instruction data
    ///
    /// * `input` - The instruction data
    fn unpack_u64(input: &[u8]) -> Result<(u64, &[u8]), BotError> {
        if input.len() >= 8 {
            let (amount, rest) = input.split_at(8);
            let amount = amount
                .get(..8)
                .and_then(|slice| slice.try_into().ok())
                .map(u64::from_le_bytes)
                .ok_or(BotError::InvalidInstructionData)?;
            Ok((amount, rest))
        } else {
            Err(BotError::InvalidInstructionData.into())
        }
    }

    /// Extracts the Raydium instruction accounts from the Raydium swap instruction.
    /// Based on https://github.com/raydium-io/raydium-amm/blob/master/program/src/processor.rs#L2242
    ///
    /// * `raydium_swap_instruction` - The Raydium swap instruction
    fn get_accounts(raydium_swap_instruction: &Value) -> Option<RaydiumAccounts> {
        if let Some(accounts) = raydium_swap_instruction["accounts"].as_array() {
            // account[0] - Token Program
            let amm_id = Self::pubkey_to_string(&accounts[1]);
            // account[2] - Raydium Authority V4
            let amm_open_orders = Self::pubkey_to_string(&accounts[3]);
            let mut k = 3;
            let mut amm_target_orders = Pubkey::default().to_string();

            if accounts.len() == RAYDIUM_ACCOUNTS_LEN_SWAP_BASE_IN + 1 {
                k = 4;
                amm_target_orders = Self::pubkey_to_string(&accounts[k]);
            }

            let pool_coin_token_account = Self::pubkey_to_string(&accounts[k + 1]);
            let pool_pc_token_account = Self::pubkey_to_string(&accounts[k + 2]);
            // account[7] - Serum Program (Open Book)
            let serum_market = Self::pubkey_to_string(&accounts[k + 4]);
            let serum_bids = Self::pubkey_to_string(&accounts[k + 5]);
            let serum_asks = Self::pubkey_to_string(&accounts[k + 6]);
            let serum_event_queue = Self::pubkey_to_string(&accounts[k + 7]);
            let serum_coin_vault = Self::pubkey_to_string(&accounts[k + 8]);
            let serum_pc_vault = Self::pubkey_to_string(&accounts[k + 9]);
            let serum_vault_signer = Self::pubkey_to_string(&accounts[k + 10]);
            let user_source_token_account = Self::pubkey_to_string(&accounts[k + 11]);
            let user_destination_token_account = Self::pubkey_to_string(&accounts[k + 12]);
            let user_source_owner = Self::pubkey_to_string(&accounts[k + 13]);

            Some(RaydiumAccounts {
                amm_id,
                amm_open_orders,
                amm_target_orders,
                pool_coin_token_account,
                pool_pc_token_account,
                serum_market,
                serum_bids,
                serum_asks,
                serum_event_queue,
                serum_coin_vault,
                serum_pc_vault,
                serum_vault_signer,
                user_source_token_account,
                user_destination_token_account,
                user_source_owner,
            })
        } else {
            None
        }
    }

    /// Finds the Raydium swap instruction in the transaction,
    /// either as a main instruction or as an inner instruction.
    fn get_raydium_swap_instruction(json: &Value) -> Option<&Value> {
        // Helper closure to check if instruction is a Raydium swap
        let is_raydium_swap = |instruction: &Value| {
            instruction["programId"]
                .as_str()
                .map(|id| id == RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM.to_string())
                .unwrap_or(false)
        };

        // Check main instructions
        let instructions = match json["params"]["result"]["transaction"]["transaction"]["message"]
            ["instructions"]
            .as_array()
        {
            Some(instructions) => instructions.iter().find(|&ix| is_raydium_swap(ix)),
            None => return None,
        };

        if instructions.is_some() {
            return instructions;
        }

        // Check inner instructions
        let inner_instructions =
            match json["params"]["result"]["transaction"]["meta"]["innerInstructions"].as_array() {
                Some(inner_instructions) => inner_instructions.iter().find_map(|inner_ix_group| {
                    match inner_ix_group["instructions"].as_array() {
                        Some(instructions) => instructions.iter().find(|&ix| is_raydium_swap(ix)),
                        None => return None,
                    }
                }),
                None => return None,
            };

        if inner_instructions.is_some() {
            return inner_instructions;
        }

        None
    }

    /// Returns a formatted string containing transaction details
    pub fn format_tx_info(&self) -> String {
        // Use if let to safely access all required data
        if let (Some(trade_data), Some(ix_data), Some(inner_ix), Some(sig)) = (
            &self.meme_trade_data,
            &self.ix_data,
            &self.inner_ix_data,
            &self.signature,
        ) {
            let operation = if trade_data.operation == BOT_RAYDIUM_OPERATION_BUY {
                "Buy"
            } else {
                "Sell"
            };

            let instruction = match ix_data {
                RaydiumSwapData::BaseIn(_) => "SwapBaseIn",
                RaydiumSwapData::BaseOut(_) => "SwapBaseOut",
            };

            format!(
                "{} | Mint: {} | Instruction: {} | Source Amount: {} | Destination Amount: {} | TX: https://solscan.io/tx/{}",
                operation,
                trade_data.meme_mint,
                instruction,
                inner_ix.source_amount,
                inner_ix.dest_amount,
                sig
            )
        } else {
            "RaydiumMemeTx: Error formatting transaction data".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod through_jupiter_aggregator {
        use super::*;

        #[test]
        fn test_get_raydium_swap_instruction() {
            let json: Value = from_str(TEST_JSON_THROUGH_JUPITER_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json);
            assert!(swap_ix.is_some());
        }

        #[test]
        fn test_get_accounts() {
            let json: Value = from_str(TEST_JSON_THROUGH_JUPITER_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix);
            assert!(accounts.is_some());
        }

        #[test]
        fn test_get_amounts() {
            let json: Value = from_str(TEST_JSON_THROUGH_JUPITER_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix).unwrap();
            let amounts = RaydiumMemeTx::get_amounts(&json, &accounts);
            assert!(amounts.is_some());
        }
    }

    mod through_okdex_aggregator {
        use super::*;

        #[test]
        fn test_get_raydium_swap_instruction() {
            let json: Value = from_str(TEST_JSON_THROUGH_OKDEX_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json);
            assert!(swap_ix.is_some());
        }

        #[test]
        fn test_get_accounts() {
            let json: Value = from_str(TEST_JSON_THROUGH_OKDEX_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix);
            assert!(accounts.is_some());
        }

        #[test]
        fn test_get_amounts() {
            let json: Value = from_str(TEST_JSON_THROUGH_OKDEX_AGGREGATOR).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix).unwrap();
            let amounts = RaydiumMemeTx::get_amounts(&json, &accounts);
            assert!(amounts.is_some());
        }
    }

    mod raydium_direct {
        use super::*;

        #[test]
        fn test_get_raydium_swap_instruction() {
            let json: Value = from_str(TEST_JSON_RAYDIUM_DIRECT).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json);
            assert!(swap_ix.is_some());
        }

        #[test]
        fn test_get_accounts() {
            let json: Value = from_str(TEST_JSON_RAYDIUM_DIRECT).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix);
            assert!(accounts.is_some());
        }

        #[test]
        fn test_get_amounts() {
            let json: Value = from_str(TEST_JSON_RAYDIUM_DIRECT).unwrap();
            let swap_ix = RaydiumMemeTx::get_raydium_swap_instruction(&json).unwrap();
            let accounts = RaydiumMemeTx::get_accounts(swap_ix).unwrap();
            let amounts = RaydiumMemeTx::get_amounts(&json, &accounts);
            assert!(amounts.is_some());
        }
    }

    #[test]
    fn test_unpack_u64() {
        let input = vec![1, 0, 0, 0, 0, 0, 0, 0, 2, 3];
        let (amount, rest) = RaydiumMemeTx::unpack_u64(&input).unwrap();
        assert_eq!(amount, 1);
        assert_eq!(rest, &[2, 3]);
    }

    #[test]
    fn test_get_instruction_data() {
        // Test SwapBaseIn instruction
        let input = bs58::encode(vec![
            RAYDIUM_SWAP_BASE_IN_INSTRUCTION,
            1,
            0,
            0,
            0,
            0,
            0,
            0,
            0, // amount_in
            2,
            0,
            0,
            0,
            0,
            0,
            0,
            0, // minimum_amount_out
        ])
        .into_string();

        let result = RaydiumMemeTx::get_instruction_data(&input);
        assert!(matches!(result, Some(RaydiumSwapData::BaseIn(_))));
    }

    const TEST_JSON_THROUGH_JUPITER_AGGREGATOR: &str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":8217137032096876,"result":{"transaction":{"transaction":{"signatures":["4CzJVBWUoMfFCyYVcLFWzd9vmi1oRnCC3x4LZQKiH3KjBzxtkJEn8o9YsKZURKRtnKUHT6QSinLXp3KeTgKq3K1k"],"message":{"accountKeys":[{"pubkey":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","writable":true,"signer":true,"source":"transaction"},{"pubkey":"iab4NSVEAztnQnNAoGFDUoCtq85LsKx14ucKZAhpXFp","writable":true,"signer":false,"source":"transaction"},{"pubkey":"3ygb59EqRLxJScSjM8JFRKFkTLUkJNETxnNJe6SiEWkZ","writable":true,"signer":false,"source":"transaction"},{"pubkey":"7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","writable":true,"signer":false,"source":"transaction"},{"pubkey":"8NSu3o4EcDpqoYnooNBxATk9aDt16KyQwzJzf8NecUVa","writable":true,"signer":false,"source":"transaction"},{"pubkey":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","writable":true,"signer":false,"source":"transaction"},{"pubkey":"11111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ComputeBudget111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","writable":false,"signer":false,"source":"transaction"},{"pubkey":"So11111111111111111111111111111111111111112","writable":false,"signer":false,"source":"transaction"},{"pubkey":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","writable":false,"signer":false,"source":"transaction"},{"pubkey":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","writable":false,"signer":false,"source":"transaction"},{"pubkey":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","writable":false,"signer":false,"source":"transaction"},{"pubkey":"D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf","writable":false,"signer":false,"source":"transaction"},{"pubkey":"HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","writable":false,"signer":false,"source":"transaction"}],"recentBlockhash":"Gqs2kw4Ayn4BW6rkbv3sbjshMN5yZz7aTg3EzUbVHo4t","instructions":[{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"GkYqiF","stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"3HwwerwVLKw5","stackHeight":null},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","lamports":34696444,"source":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ"},"type":"transfer"},"stackHeight":null},{"programId":"JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","accounts":["8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","So11111111111111111111111111111111111111112","TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","11111111111111111111111111111111"],"data":"2tDqDdUmhLW1t","stackHeight":null},{"programId":"JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","accounts":["TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","3ygb59EqRLxJScSjM8JFRKFkTLUkJNETxnNJe6SiEWkZ","JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf","JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","iab4NSVEAztnQnNAoGFDUoCtq85LsKx14ucKZAhpXFp","8NSu3o4EcDpqoYnooNBxATk9aDt16KyQwzJzf8NecUVa","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","3ygb59EqRLxJScSjM8JFRKFkTLUkJNETxnNJe6SiEWkZ","9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ"],"data":"PrpFmsY4d26dKbdKMAXs4ngaKLh1xBT9Htah4uvabDrwSa31","stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","destination":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","owner":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ"},"type":"closeAccount"},"stackHeight":null}],"addressTableLookups":[]}},"meta":{"err":null,"status":{"Ok":null},"fee":14522,"preBalances":[65314327,87226878765,2039280,6124800,2039280,0,1,1,1141440,742953189685,934087680,13532449906,1141440,0,1461600],"postBalances":[32642641,87259535929,2039280,6124800,2039280,0,1,1,1141440,742953189685,934087680,13532449906,1141440,0,1461600],"innerInstructions":[{"index":3,"instructions":[{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"account":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","space":165},"type":"allocate"},"stackHeight":2},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"account":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},"type":"assign"},"stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","mint":"So11111111111111111111111111111111111111112","owner":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ"},"type":"initializeAccount3"},"stackHeight":2}]},{"index":4,"instructions":[{"programId":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","accounts":["TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","iab4NSVEAztnQnNAoGFDUoCtq85LsKx14ucKZAhpXFp","8NSu3o4EcDpqoYnooNBxATk9aDt16KyQwzJzf8NecUVa","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","7rKBZVaas5qjWFMsAJryNH7iUb12FKEWQCccZi7KRr45","8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM","3ygb59EqRLxJScSjM8JFRKFkTLUkJNETxnNJe6SiEWkZ","9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ"],"data":"5w2daniZhbmKXT4KH8z8qAo","stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"32657164","authority":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","destination":"iab4NSVEAztnQnNAoGFDUoCtq85LsKx14ucKZAhpXFp","source":"8gdnTCgXmh4D7WigAZd3nx7ee59Kx2CKrBLC3LrTPNvM"},"type":"transfer"},"stackHeight":3},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"70119143270","authority":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","destination":"3ygb59EqRLxJScSjM8JFRKFkTLUkJNETxnNJe6SiEWkZ","source":"8NSu3o4EcDpqoYnooNBxATk9aDt16KyQwzJzf8NecUVa"},"type":"transfer"},"stackHeight":3},{"programId":"JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4","accounts":["D8cy77BBepLMngZx6ZukaTff5hCt1HrWyKk3Hnd9oitf"],"data":"QMqFu4fYGGeUEysFnenhAvR83g86EDDNxzUskfkWKYCBPWe1hqgD6jgKAXr6aYoEQaxoqYMTvWgPVk2AHWGHjdbNiNtoaPfZA4znu6cRUSWSeJgWAj3zWAFTrnaQgQ6S8LkNt5J5iVLCdcS5J14G3eYKhLwRfqGS5mgnDRqCKxg8Vmq","stackHeight":2}]}],"logMessages":["Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 invoke [1]","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: InitializeAccount3","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3158 of 70818 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 consumed 11005 of 78502 compute units","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 success","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 invoke [1]","Program log: Instruction: Route","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 invoke [2]","Program log: ray_log: AwxP8gEAAAAAAAAAAAAAAAACAAAAAAAAAAxP8gEAAAAAPW0BTxQAAADQhkfU0qoAAGY3bVMQAAAA","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [3]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4736 of 36103 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [3]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4645 of 28386 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 consumed 30499 of 53385 compute units","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 success","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 invoke [2]","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 consumed 471 of 20512 compute units","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 success","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 consumed 49591 of 67497 compute units","Program return: JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 ZjdtUxAAAAA=","Program JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: CloseAccount","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 2915 of 17906 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success"],"preTokenBalances":[{"accountIndex":1,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":87.224839485,"decimals":9,"amount":"87224839485","uiAmountString":"87.224839485"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":2,"mint":"HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":4,"mint":"HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","uiTokenAmount":{"uiAmount":187822481.31144,"decimals":6,"amount":"187822481311440","uiAmountString":"187822481.31144"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"postTokenBalances":[{"accountIndex":1,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":87.257496649,"decimals":9,"amount":"87257496649","uiAmountString":"87.257496649"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":2,"mint":"HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","uiTokenAmount":{"uiAmount":70119.14327,"decimals":6,"amount":"70119143270","uiAmountString":"70119.14327"},"owner":"9dVoKCQjWfy9cRZN81ApLHzhTbLWydhu9nQov1yHd7XQ","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":4,"mint":"HE1DRiix2wR3BcxhQbwZnRyYDzJxfmFUZ3A3ztEApump","uiTokenAmount":{"uiAmount":187752362.16817,"decimals":6,"amount":"187752362168170","uiAmountString":"187752362.16817"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"rewards":[],"computeUnitsConsumed":63961},"version":0},"signature":"4CzJVBWUoMfFCyYVcLFWzd9vmi1oRnCC3x4LZQKiH3KjBzxtkJEn8o9YsKZURKRtnKUHT6QSinLXp3KeTgKq3K1k","slot":305736842}}}"#;

    const TEST_JSON_THROUGH_OKDEX_AGGREGATOR: &str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":6107525713782463,"result":{"transaction":{"transaction":{"signatures":["45913tfZvYNa3LEUSDJU2qAkFHC1AzyPigzAz3NxFVm7q5W9TjR5Ww6RhQbLDvWNCK3CaLyWGeSccx6ktNEiDt3U"],"message":{"accountKeys":[{"pubkey":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","writable":true,"signer":true,"source":"transaction"},{"pubkey":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","writable":true,"signer":false,"source":"transaction"},{"pubkey":"AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","writable":true,"signer":false,"source":"transaction"},{"pubkey":"25mYnjJ2MXHZH6NvTTdA63JvjgRVcuiaj6MRiEQNs1Dq","writable":true,"signer":false,"source":"transaction"},{"pubkey":"HRWPXWC3ZGf4mGQgDJvPfzHY4e8GyTy6VPtdZSo6j4K7","writable":true,"signer":false,"source":"transaction"},{"pubkey":"CqmnVqAQm5kSztq9HDRM8PZYW7TJbcKCRiRXK8BVM4tg","writable":true,"signer":false,"source":"transaction"},{"pubkey":"H1eyzdEvA5iZa8c6XbCMPik8EJ5v4objGCu8QhihtCw3","writable":true,"signer":false,"source":"transaction"},{"pubkey":"C52Yxg2Kh4G7iUHLhn4VsvAaS9EVWZ62drc2g3gzson7","writable":true,"signer":false,"source":"transaction"},{"pubkey":"Ari3uBoU8tNXNLa5EL1TyVPABEZndSfMHLMRZNv4iMmR","writable":true,"signer":false,"source":"transaction"},{"pubkey":"5tRo5jeqjkrAedsmkDRc4yVBBqrQXK3hNbF5ru5rqSRW","writable":true,"signer":false,"source":"transaction"},{"pubkey":"E5iHMbBKmv7Hh4Wu4zdKm8gVbvGDQrqVXGJBhDRdfKXx","writable":true,"signer":false,"source":"transaction"},{"pubkey":"CXRUZz9CoqRuKJqwSQo4anCP2LRSh3sw37wVR8EiNiZ6","writable":true,"signer":false,"source":"transaction"},{"pubkey":"3M21bTc9oMvJ2iBTqh4QapQabLCwMWJ9SnCh9m6d2Axi","writable":true,"signer":false,"source":"transaction"},{"pubkey":"AewKEnzf2X28Xp6gpPzRQkBMZQeyfhJyQjA9TTU57arU","writable":true,"signer":false,"source":"transaction"},{"pubkey":"4JSdYj9Y8auJBq4G68pZhqZxPpcPnCjFYc5ux9GfB7fK","writable":true,"signer":false,"source":"transaction"},{"pubkey":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","writable":false,"signer":false,"source":"transaction"},{"pubkey":"11111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ComputeBudget111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","writable":false,"signer":false,"source":"transaction"},{"pubkey":"6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma","writable":false,"signer":false,"source":"transaction"},{"pubkey":"So11111111111111111111111111111111111111112","writable":false,"signer":false,"source":"lookupTable"},{"pubkey":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","writable":false,"signer":false,"source":"lookupTable"},{"pubkey":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","writable":false,"signer":false,"source":"lookupTable"},{"pubkey":"SysvarRent111111111111111111111111111111111","writable":false,"signer":false,"source":"lookupTable"},{"pubkey":"srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX","writable":false,"signer":false,"source":"lookupTable"}],"recentBlockhash":"8PT3oVp8QPtr8orTUMSkFxMf3rYUJP3jXczshrUUsFWD","instructions":[{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"FXSo5R","stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"3iHy9czuofXD","stackHeight":null},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"base":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","lamports":2039280,"newAccount":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","seed":"1733490215786","source":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","space":165},"type":"createAccountWithSeed"},"stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","mint":"So11111111111111111111111111111111111111112","owner":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","rentSysvar":"SysvarRent111111111111111111111111111111111"},"type":"initializeAccount"},"stackHeight":null},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","lamports":84722365,"source":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9"},"type":"transfer"},"stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5"},"type":"syncNative"},"stackHeight":null},{"program":"spl-associated-token-account","programId":"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","parsed":{"info":{"account":"AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","source":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","systemProgram":"11111111111111111111111111111111","tokenProgram":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","wallet":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9"},"type":"createIdempotent"},"stackHeight":null},{"programId":"6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma","accounts":["4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","So11111111111111111111111111111111111111112","3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","25mYnjJ2MXHZH6NvTTdA63JvjgRVcuiaj6MRiEQNs1Dq","11111111111111111111111111111111","675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","HRWPXWC3ZGf4mGQgDJvPfzHY4e8GyTy6VPtdZSo6j4K7","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","CqmnVqAQm5kSztq9HDRM8PZYW7TJbcKCRiRXK8BVM4tg","H1eyzdEvA5iZa8c6XbCMPik8EJ5v4objGCu8QhihtCw3","C52Yxg2Kh4G7iUHLhn4VsvAaS9EVWZ62drc2g3gzson7","Ari3uBoU8tNXNLa5EL1TyVPABEZndSfMHLMRZNv4iMmR","srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX","5tRo5jeqjkrAedsmkDRc4yVBBqrQXK3hNbF5ru5rqSRW","E5iHMbBKmv7Hh4Wu4zdKm8gVbvGDQrqVXGJBhDRdfKXx","CXRUZz9CoqRuKJqwSQo4anCP2LRSh3sw37wVR8EiNiZ6","3M21bTc9oMvJ2iBTqh4QapQabLCwMWJ9SnCh9m6d2Axi","AewKEnzf2X28Xp6gpPzRQkBMZQeyfhJyQjA9TTU57arU","4JSdYj9Y8auJBq4G68pZhqZxPpcPnCjFYc5ux9GfB7fK","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1"],"data":"8PwEjmahyKM2b7CjhAVf3huhgUweAaQBqqkwBJ7CP4CauvNXaBMjJTVLJdzbznrkp1NE7haG11qANdio8SXDg7yQ4cuVaZnUfaK9","stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","destination":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","owner":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9"},"type":"closeAccount"},"stackHeight":null}],"addressTableLookups":[{"accountKey":"FCE3BU7YpHtHg5b1nGvpjN4XXKWbrFUP9fWT2vwaQvzR","writableIndexes":[],"readonlyIndexes":[86,70,59,46,63]}]}},"meta":{"err":null,"status":{"Ok":null},"fee":76226,"preBalances":[90532959,0,2039280,13801716493409,6124800,23357760,16258560,89392748017,2039280,3591360,457104960,457104960,1825496640,2039280,2039280,1461600,1,934087680,1,731913600,1141440,742953189685,13532449906,1141440,1009200,1141440],"postBalances":[5008055,0,2039280,13801717219722,6124800,23357760,16258560,89477470382,2039280,3591360,457104960,457104960,1825496640,2039280,2039280,1461600,1,934087680,1,731913600,1141440,742953189685,13532449906,1141440,1009200,1141440],"innerInstructions":[{"index":7,"instructions":[{"programId":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","accounts":["TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","HRWPXWC3ZGf4mGQgDJvPfzHY4e8GyTy6VPtdZSo6j4K7","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","CqmnVqAQm5kSztq9HDRM8PZYW7TJbcKCRiRXK8BVM4tg","H1eyzdEvA5iZa8c6XbCMPik8EJ5v4objGCu8QhihtCw3","C52Yxg2Kh4G7iUHLhn4VsvAaS9EVWZ62drc2g3gzson7","Ari3uBoU8tNXNLa5EL1TyVPABEZndSfMHLMRZNv4iMmR","srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX","5tRo5jeqjkrAedsmkDRc4yVBBqrQXK3hNbF5ru5rqSRW","E5iHMbBKmv7Hh4Wu4zdKm8gVbvGDQrqVXGJBhDRdfKXx","CXRUZz9CoqRuKJqwSQo4anCP2LRSh3sw37wVR8EiNiZ6","3M21bTc9oMvJ2iBTqh4QapQabLCwMWJ9SnCh9m6d2Axi","AewKEnzf2X28Xp6gpPzRQkBMZQeyfhJyQjA9TTU57arU","4JSdYj9Y8auJBq4G68pZhqZxPpcPnCjFYc5ux9GfB7fK","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9"],"data":"6JwWigTkPsZmcVvtvV1iKkK","stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"84722365","authority":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","destination":"C52Yxg2Kh4G7iUHLhn4VsvAaS9EVWZ62drc2g3gzson7","source":"3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5"},"type":"transfer"},"stackHeight":3},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"173528152404","authority":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","destination":"AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","source":"Ari3uBoU8tNXNLa5EL1TyVPABEZndSfMHLMRZNv4iMmR"},"type":"transfer"},"stackHeight":3},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"25mYnjJ2MXHZH6NvTTdA63JvjgRVcuiaj6MRiEQNs1Dq","lamports":726313,"source":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9"},"type":"transfer"},"stackHeight":2}]}],"logMessages":["Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: InitializeAccount","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3443 of 146550 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: SyncNative","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3045 of 142957 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL invoke [1]","Program log: CreateIdempotent","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL consumed 7338 of 139912 compute units","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL success","Program 6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma invoke [1]","Program log: Instruction: CommissionSolSwap2","Program log: order_id: 105213","Program log: So11111111111111111111111111111111111111112","Program log: 3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","Program log: before_source_balance: 84722365, before_destination_balance: 0, amount_in: 84722365, expect_amount_out: 177979875505, min_return: 160181887955","Program log: Dex::RaydiumSwap amount_in: 84722365, offset: 0","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 invoke [2]","Program log: ray_log: A73CDAUAAAAAAQAAAAAAAAACAAAAAAAAAL3CDAUAAAAAAfgZ0BQAAAA8HQ4rGKcAAFR5FWcoAAAA","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [3]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4736 of 80336 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [3]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4645 of 72619 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 consumed 32233 of 99150 compute units","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 success","Program data: QMbN6CYIceIEvcIMBQAAAABUeRVnKAAAAA==","Program log: SwapEvent { dex: RaydiumSwap, amount_in: 84722365, amount_out: 173528152404 }","Program log: 3wAz4mwNprxDNGtpEFbdgzfvVtpx5xKt7meEFsDENxy5","Program log: AKZ7xX9C3MvPt7B21qRHMSfXkAKc1wLpni1WsE4Cvhrc","Program log: after_source_balance: 0, after_destination_balance: 173528152404, source_token_change: 84722365, destination_token_change: 173528152404","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program log: commission_direction: true, commission_amount: 726313","Program 6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma consumed 77886 of 132574 compute units","Program return: 6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma VHkVZygAAAA=","Program 6m2CDdhRgxpH4WjvdzxAYbGxwdGUz5MziiL5jek2kBma success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: CloseAccount","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 2915 of 54688 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success"],"preTokenBalances":[{"accountIndex":2,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":7,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":89.390708737,"decimals":9,"amount":"89390708737","uiAmountString":"89.390708737"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":8,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":183722243.398972,"decimals":6,"amount":"183722243398972","uiAmountString":"183722243.398972"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":13,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":0.0,"decimals":9,"amount":"0","uiAmountString":"0"},"owner":"GB7t92nAY3QLSyJaBztc7YcCAiBKxCxQimQHwyKBZxwM","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":14,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"GB7t92nAY3QLSyJaBztc7YcCAiBKxCxQimQHwyKBZxwM","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"postTokenBalances":[{"accountIndex":2,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":173528.152404,"decimals":6,"amount":"173528152404","uiAmountString":"173528.152404"},"owner":"4cUmct5JeXGiArfeTbpJz39rQfHD4YzstUMqPaMTfad9","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":7,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":89.475431102,"decimals":9,"amount":"89475431102","uiAmountString":"89.475431102"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":8,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":183548715.246568,"decimals":6,"amount":"183548715246568","uiAmountString":"183548715.246568"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":13,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":0.0,"decimals":9,"amount":"0","uiAmountString":"0"},"owner":"GB7t92nAY3QLSyJaBztc7YcCAiBKxCxQimQHwyKBZxwM","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":14,"mint":"3WZNphshQpxWGAN9EgTgAQPj4kHkTxmSYmmH91Dupump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"GB7t92nAY3QLSyJaBztc7YcCAiBKxCxQimQHwyKBZxwM","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"rewards":[],"computeUnitsConsumed":95227},"version":0},"signature":"45913tfZvYNa3LEUSDJU2qAkFHC1AzyPigzAz3NxFVm7q5W9TjR5Ww6RhQbLDvWNCK3CaLyWGeSccx6ktNEiDt3U","slot":305770656}}}"#;

    const TEST_JSON_RAYDIUM_DIRECT: &str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":7004306216227629,"result":{"transaction":{"transaction":{"signatures":["4aEsSpvYB25WFVCfSE2PxhSAtERxUGnRGjeZHz7unPoQgX5e1KdDMdC8rmyx8btGkiSagENvY6L4FCfvbfzWSiLW"],"message":{"accountKeys":[{"pubkey":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","writable":true,"signer":true,"source":"transaction"},{"pubkey":"889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np","writable":true,"signer":false,"source":"transaction"},{"pubkey":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","writable":true,"signer":false,"source":"transaction"},{"pubkey":"E77wHMJyMDCoDGUNCefq5pJy71nCc1ccHJcM498WyE1K","writable":true,"signer":false,"source":"transaction"},{"pubkey":"DUL6JuyHWi94ginWwx3eYctHENfacxLqLhWyDKqQFQjT","writable":true,"signer":false,"source":"transaction"},{"pubkey":"E99mHFsPEt3Tyuy7jpRefGURKTppTL82VBecAaEJazuR","writable":true,"signer":false,"source":"transaction"},{"pubkey":"5rQyXNJHoZnWygAMyK5gpQjcL6YfweML3yfiX3sZa2Vz","writable":true,"signer":false,"source":"transaction"},{"pubkey":"4przLqduZp7LgSARBwJHEkjrPQSRBXbukeGQnBYpkcf8","writable":true,"signer":false,"source":"transaction"},{"pubkey":"Gpiauqqx1WBFKorVf6ZkJqtc654R6Hs64Ut4ics7ybMh","writable":true,"signer":false,"source":"transaction"},{"pubkey":"8fVe9Sy5sEb7casznTAzyNt7eNhW77esP2ah8xMVChmP","writable":true,"signer":false,"source":"transaction"},{"pubkey":"GJHMsqkGpRxgadATX3R1roDNWPvWjCjbiMEsyEr971Nv","writable":true,"signer":false,"source":"transaction"},{"pubkey":"fUSVxvCoFSHaqriJqr5PU218N5Ewwqq5UQXB8hbLFdD","writable":true,"signer":false,"source":"transaction"},{"pubkey":"5pJmGEiu4BfhpNJ2DGCDQc7yimR1tpHjFFhYY61BbGFe","writable":true,"signer":false,"source":"transaction"},{"pubkey":"8wefFUj1cSDJ7VrWptyrF9TQrDWBLMU9XVVEgCbNmtDF","writable":true,"signer":false,"source":"transaction"},{"pubkey":"7CifH4CWTNLJzeZgP271pYHGDqMN6gmqqGC2ha9KZFZn","writable":true,"signer":false,"source":"transaction"},{"pubkey":"11111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","writable":false,"signer":false,"source":"transaction"},{"pubkey":"So11111111111111111111111111111111111111112","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","writable":false,"signer":false,"source":"transaction"},{"pubkey":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","writable":false,"signer":false,"source":"transaction"},{"pubkey":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","writable":false,"signer":false,"source":"transaction"},{"pubkey":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","writable":false,"signer":false,"source":"transaction"},{"pubkey":"Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ComputeBudget111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"SysvarRent111111111111111111111111111111111","writable":false,"signer":false,"source":"lookupTable"},{"pubkey":"srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX","writable":false,"signer":false,"source":"lookupTable"}],"recentBlockhash":"GutPYptJjfhrfx9DGUBqi9GUp6Mw21bnynNWK3rk2gLs","instructions":[{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"base":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","lamports":202039280,"newAccount":"889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np","owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","seed":"hFcsfpQxLJMAmAp2scHnLoQ38DfibZp9","source":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","space":165},"type":"createAccountWithSeed"},"stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np","mint":"So11111111111111111111111111111111111111112","owner":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","rentSysvar":"SysvarRent111111111111111111111111111111111"},"type":"initializeAccount"},"stackHeight":null},{"program":"spl-associated-token-account","programId":"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","parsed":{"info":{"account":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","source":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","systemProgram":"11111111111111111111111111111111","tokenProgram":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","wallet":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5"},"type":"create"},"stackHeight":null},{"programId":"675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8","accounts":["TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","E77wHMJyMDCoDGUNCefq5pJy71nCc1ccHJcM498WyE1K","5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","DUL6JuyHWi94ginWwx3eYctHENfacxLqLhWyDKqQFQjT","E99mHFsPEt3Tyuy7jpRefGURKTppTL82VBecAaEJazuR","5rQyXNJHoZnWygAMyK5gpQjcL6YfweML3yfiX3sZa2Vz","4przLqduZp7LgSARBwJHEkjrPQSRBXbukeGQnBYpkcf8","srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX","Gpiauqqx1WBFKorVf6ZkJqtc654R6Hs64Ut4ics7ybMh","8fVe9Sy5sEb7casznTAzyNt7eNhW77esP2ah8xMVChmP","GJHMsqkGpRxgadATX3R1roDNWPvWjCjbiMEsyEr971Nv","fUSVxvCoFSHaqriJqr5PU218N5Ewwqq5UQXB8hbLFdD","5pJmGEiu4BfhpNJ2DGCDQc7yimR1tpHjFFhYY61BbGFe","8wefFUj1cSDJ7VrWptyrF9TQrDWBLMU9XVVEgCbNmtDF","Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np","urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5"],"data":"5ubuLEe9Mv9sNr92oJz23Fu","stackHeight":null},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np","destination":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","owner":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5"},"type":"closeAccount"},"stackHeight":null},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"7CifH4CWTNLJzeZgP271pYHGDqMN6gmqqGC2ha9KZFZn","lamports":2000000,"source":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5"},"type":"transfer"},"stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"Kq1GWK","stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"3FDqrcm6jSXH","stackHeight":null}],"addressTableLookups":[{"accountKey":"2immgwYNHBbyVQKVGCEkgWpi53bLwWNRMB5G2nbgYV17","writableIndexes":[],"readonlyIndexes":[5,11]}]}},"meta":{"err":null,"status":{"Ok":null},"fee":55000,"preBalances":[863036618,0,0,6124800,23357760,16258560,42175535548,2039280,3591360,457104960,457104960,1825496640,2039280,2039280,2172483553846,1,934087680,742953189685,731913600,1461600,1141440,13532449906,0,1,1009200,1141440],"postBalances":[658942338,0,2039280,6124800,23357760,16258560,42375535548,2039280,3591360,457104960,457104960,1825496640,2039280,2039280,2172485553846,1,934087680,742953189685,731913600,1461600,1141440,13532449906,0,1,1009200,1141440],"innerInstructions":[{"index":2,"instructions":[{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"extensionTypes":["immutableOwner"],"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump"},"type":"getAccountDataSize"},"stackHeight":2},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"lamports":2039280,"newAccount":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","source":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","space":165},"type":"createAccount"},"stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy"},"type":"initializeImmutableOwner"},"stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"account":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","owner":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5"},"type":"initializeAccount3"},"stackHeight":2}]},{"index":3,"instructions":[{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"200000000","authority":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","destination":"5rQyXNJHoZnWygAMyK5gpQjcL6YfweML3yfiX3sZa2Vz","source":"889uT38bWbxQgnoUAL3CcjNJ5FuobH6tpkR4HMT8v5np"},"type":"transfer"},"stackHeight":2},{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"1932854460806","authority":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","destination":"urU53gb4Rbdg611pGsfF17yttNvo4PYwTF8b82nMigy","source":"4przLqduZp7LgSARBwJHEkjrPQSRBXbukeGQnBYpkcf8"},"type":"transfer"},"stackHeight":2}]}],"logMessages":["Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: InitializeAccount","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 3443 of 299850 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL invoke [1]","Program log: Create","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: GetAccountDataSize","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 1569 of 291040 compute units","Program return: TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA pQAAAAAAAAA=","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program log: Initialize the associated token account","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: InitializeImmutableOwner","Program log: Please upgrade to SPL Token 2022 for immutable owner support","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 1405 of 284453 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: InitializeAccount3","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4188 of 280571 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL consumed 20307 of 296407 compute units","Program ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL success","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 invoke [1]","Program log: ray_log: AwDC6wsAAAAAHPQSyS4BAAACAAAAAAAAAADC6wsAAAAAzHu80QkAAAD8M6cUYHUBAIaBGgfCAQAA","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4736 of 257966 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4645 of 250249 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 consumed 31574 of 276100 compute units","Program 675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 success","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]","Program log: Instruction: CloseAccount","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 2915 of 244526 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success"],"preTokenBalances":[{"accountIndex":6,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":42.173496268,"decimals":9,"amount":"42173496268","uiAmountString":"42.173496268"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":7,"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","uiTokenAmount":{"uiAmount":410530500.523004,"decimals":6,"amount":"410530500523004","uiAmountString":"410530500.523004"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":12,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":0.0,"decimals":9,"amount":"0","uiAmountString":"0"},"owner":"Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":13,"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"postTokenBalances":[{"accountIndex":2,"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","uiTokenAmount":{"uiAmount":1932854.460806,"decimals":6,"amount":"1932854460806","uiAmountString":"1932854.460806"},"owner":"9txcdTtZaHUsT55kKmgivkfcdnP4tGppqN7tUAjpjVj5","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":6,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":42.373496268,"decimals":9,"amount":"42373496268","uiAmountString":"42.373496268"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":7,"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","uiTokenAmount":{"uiAmount":408597646.062198,"decimals":6,"amount":"408597646062198","uiAmountString":"408597646.062198"},"owner":"5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":12,"mint":"So11111111111111111111111111111111111111112","uiTokenAmount":{"uiAmount":0.0,"decimals":9,"amount":"0","uiAmountString":"0"},"owner":"Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":13,"mint":"3sgaCyXJTAWxgaFXU6CLDjo1dg8Z8GLGdjDr8wBmpump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"Gf2r36HC4FgcFGSzx4odsrYV4zaVoFgWK4wfyAmVtqRK","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"rewards":[],"computeUnitsConsumed":58839},"version":0},"signature":"4aEsSpvYB25WFVCfSE2PxhSAtERxUGnRGjeZHz7unPoQgX5e1KdDMdC8rmyx8btGkiSagENvY6L4FCfvbfzWSiLW","slot":305781709}}}"#;
}
