use helius::Helius;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use std::{error::Error, sync::Arc};

use crate::constants::{
    BOT_RAYDIUM_OPERATION_BUY, BOT_RAYDIUM_OPERATION_SELL, RAYDIUM_AMM_AUTHORITY,
    RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM, SERUM_PROGRAM, WSOL_MINT,
};

use super::{tx_common::SendSmartTx, RaydiumMemeTx, RaydiumSwapData};

pub struct RaydiumMemeTxSend {}

impl SendSmartTx for RaydiumMemeTxSend {}

impl RaydiumMemeTxSend {
    pub async fn compose_and_send(
        helius: Arc<Helius>,
        signer_prv_key: Arc<String>,
        max_sol_buy: u64,
        slippage_percent: u64,
        payload: String,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let raydium_meme_tx = RaydiumMemeTx::new(payload, Arc::clone(&helius)).await;
        println!("{}", raydium_meme_tx.format_tx_info());

        let meme_trade_data = raydium_meme_tx
            .meme_trade_data
            .as_ref()
            .ok_or("No meme trade data")?;

        if meme_trade_data.operation == BOT_RAYDIUM_OPERATION_SELL {
            return Err("Sell operation is not supported".into());
        }

        let mut instructions: Vec<Instruction> = vec![];

        let signer = Keypair::from_base58_string(signer_prv_key.as_str());

        let user_source_owner = signer.try_pubkey().expect("Cannot get signer pubkey");

        let accounts = raydium_meme_tx
            .accounts
            .as_ref()
            .ok_or("No meme accounts")?;

        let ix_data = raydium_meme_tx
            .ix_data
            .as_ref()
            .ok_or("No instruction data")?;

        let inner_ix_data = raydium_meme_tx
            .inner_ix_data
            .as_ref()
            .ok_or("No innner instructions data")?;

        // let source_amount = inner_ix_data.source_amount;
        // let dest_amount = inner_ix_data.dest_amount;

        // Get user source token account
        let user_source_token_account = match meme_trade_data.operation {
            BOT_RAYDIUM_OPERATION_BUY => {
                get_associated_token_address(&user_source_owner, &WSOL_MINT)
            }
            BOT_RAYDIUM_OPERATION_SELL => {
                get_associated_token_address(&user_source_owner, &meme_trade_data.meme_mint)
            }
            _ => {
                return Err("Unknown operation".into());
            }
        };

        // Get user destination token account
        let user_destinationm_token_account = match meme_trade_data.operation {
            BOT_RAYDIUM_OPERATION_BUY => {
                get_associated_token_address(&user_source_owner, &meme_trade_data.meme_mint)
            }
            BOT_RAYDIUM_OPERATION_SELL => {
                get_associated_token_address(&user_source_owner, &WSOL_MINT)
            }
            _ => {
                return Err("Unknown operation".into());
            }
        };

        // Create associated token account for user source token if needed
        if meme_trade_data.operation == BOT_RAYDIUM_OPERATION_BUY {
            instructions.push(create_associated_token_account_idempotent(
                &user_source_owner,
                &user_source_owner,
                &meme_trade_data.meme_mint,
                &spl_token::id(),
            ));
        } else if meme_trade_data.operation == BOT_RAYDIUM_OPERATION_SELL {
            // TODO: Create associated token account for WSOL
            // But for now, we decided there is no need to create it
            // as the user should have it already because
            // copy trading starts with WSOL.
        }

        let mut data: Vec<u8> = Vec::new();
        match ix_data {
            RaydiumSwapData::BaseIn(swap) => {
                data.extend(vec![swap.instruction]);
            }
            RaydiumSwapData::BaseOut(swap) => {
                data.extend(vec![swap.instruction]);
            }
        }

        data.extend(max_sol_buy.to_le_bytes());
        data.extend(0u64.to_le_bytes());

        // Helper function to create AccountMeta
        let writeable = |pubkey: &str| AccountMeta::new(Pubkey::from_str_const(pubkey), false);
        let readonly = |pubkey: Pubkey| AccountMeta::new_readonly(pubkey, false);

        // Base accounts that are always required
        let mut raydium_swap_accounts = vec![
            readonly(spl_token::id()),            // #1 Token Program
            writeable(&accounts.amm_id),          // #2 Amm Id
            readonly(RAYDIUM_AMM_AUTHORITY),      // #3 Amm Authority
            writeable(&accounts.amm_open_orders), // #4 Amm Open Orders
        ];

        // Optional Target Orders account
        if !accounts
            .amm_target_orders
            .eq(&Pubkey::default().to_string())
        {
            raydium_swap_accounts.push(writeable(&accounts.amm_target_orders)); // #5 Amm Target Orders
        }

        // Pool and Serum accounts
        let pool_and_serum = vec![
            writeable(&accounts.pool_coin_token_account), // #5/6 Pool Coin Token Account
            writeable(&accounts.pool_pc_token_account),   // #6/7 Pool Pc Token Account
            writeable(&SERUM_PROGRAM.to_string()),        // #7/8 Serum Program Id
            writeable(&accounts.serum_market),            // #8/9 Serum Market
            writeable(&accounts.serum_bids),              // #9/10 Serum Bids
            writeable(&accounts.serum_asks),              // #10/11 Serum Asks
            writeable(&accounts.serum_event_queue),       // #11/12 Serum Event Queue
            writeable(&accounts.serum_coin_vault),        // #12/13 Serum Coin Vault
            writeable(&accounts.serum_pc_vault),          // #13/14 Serum Pc Vault
            writeable(&accounts.serum_vault_signer),      // #14/15 Serum Vault Signer
        ];

        // User accounts
        let user_accounts = vec![
            AccountMeta::new(user_source_token_account, false), // #15/16 User Source Token Account
            AccountMeta::new(user_destinationm_token_account, false), // #16/17 User Destination Token Account
            AccountMeta::new(user_source_owner, true),                // #17/18 User Source Owner
        ];

        raydium_swap_accounts.extend(pool_and_serum);
        raydium_swap_accounts.extend(user_accounts);

        let raydium_swap_ix = Instruction {
            program_id: RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM,
            accounts: raydium_swap_accounts,
            data,
        };
        instructions.push(raydium_swap_ix);

        Self::bfg9000_send_smart_tx(
            helius,
            instructions,
            Some(raydium_meme_tx.compute_unit_limit),
            None,
            signer_prv_key.as_str().to_string(),
            raydium_meme_tx.signature,
        )
        .await
    }
}
