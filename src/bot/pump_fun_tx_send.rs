use std::{error::Error, sync::Arc};

use helius::Helius;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    system_program,
};
use spl_associated_token_account::{
    get_associated_token_address, instruction::create_associated_token_account_idempotent,
};
use tokio::sync::Mutex;

use crate::constants::*;

use super::{tx_common::SendSmartTx, PumpFunTx};

pub struct PumpFunTxSend {}

impl SendSmartTx for PumpFunTxSend {}

impl PumpFunTxSend {
    pub async fn buy(
        helius: Arc<Mutex<Helius>>,
        signer_prv_key: Arc<String>,
        bonding_curve: Pubkey,
        associated_bonding_curve: Pubkey,
        mint: Pubkey,
        decimals: u32,
        price_in_sol_lamports: f64,
        max_sol_lamports: u64,
        slippage_percent: u64,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let signer = Keypair::from_base58_string(signer_prv_key.as_str());
        let signer_pub_key = signer.try_pubkey().unwrap();
        let user_ata = get_associated_token_address(&signer_pub_key, &mint);

        // Prepare buy instruction data
        let mut data: Vec<u8> = PUMP_FUN_ACTION_BUY.to_vec();
        let amount = (max_sol_lamports as f64 / price_in_sol_lamports) as u64;
        let amount_with_decimals = amount * 10u64.pow(decimals as u32);
        println!(
            "Amount: {:?}, Amount with decimals: {:?}",
            amount, amount_with_decimals
        );
        data.extend_from_slice(&amount_with_decimals.to_le_bytes());

        let fees = (max_sol_lamports as f64 * PUMP_FUN_FEES).round() as u64;
        let slippage = (max_sol_lamports as f64 * slippage_percent as f64 / 100.0).round() as u64;
        let final_max_sol_buy = max_sol_lamports + slippage + fees;
        println!("Final max sol buy: {:?}", final_max_sol_buy);
        data.extend_from_slice(&final_max_sol_buy.to_le_bytes());

        Self::send_pump_fun_tx(
            helius,
            signer_prv_key,
            mint,
            bonding_curve,
            associated_bonding_curve,
            user_ata,
            data,
            true, // is_buy
        )
        .await
    }

    async fn send_pump_fun_tx(
        helius: Arc<Mutex<Helius>>,
        signer_prv_key: Arc<String>,
        mint: Pubkey,
        bonding_curve: Pubkey,
        associated_bonding_curve: Pubkey,
        user_ata: Pubkey,
        data: Vec<u8>,
        is_buy: bool,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let signer = Keypair::from_base58_string(signer_prv_key.as_str());
        let signer_pub_key = signer.try_pubkey().unwrap();

        let pump_fun_ix = Instruction {
            program_id: PUMP_FUN_PROGRAM,
            accounts: vec![
                AccountMeta::new_readonly(PUMP_FUN_GLOBAL, false),
                AccountMeta::new(PUMP_FUN_FEE_RECIPIENT, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(bonding_curve, false),
                AccountMeta::new(associated_bonding_curve, false),
                AccountMeta::new(user_ata, false),
                AccountMeta::new(signer_pub_key, true),
                AccountMeta::new_readonly(system_program::id(), false),
                AccountMeta::new_readonly(
                    if is_buy {
                        spl_token::id()
                    } else {
                        spl_associated_token_account::id()
                    },
                    false,
                ),
                AccountMeta::new_readonly(
                    if is_buy {
                        solana_program::sysvar::rent::ID
                    } else {
                        spl_token::id()
                    },
                    false,
                ),
                AccountMeta::new_readonly(PUMP_EVENT_AUTHORITY, false),
                AccountMeta::new_readonly(PUMP_FUN_PROGRAM, false),
            ],
            data,
        };

        let create_ata_ix = if is_buy {
            Some(create_associated_token_account_idempotent(
                &signer_pub_key,
                &signer_pub_key,
                &mint,
                &spl_token::id(),
            ))
        } else {
            None
        };

        let instructions = match create_ata_ix {
            Some(create_ata_ix) => vec![create_ata_ix, pump_fun_ix],
            None => vec![pump_fun_ix],
        };

        Self::bfg9000_send_smart_tx(
            helius,
            instructions,
            Some(DEFAULT_COMPUTE_UNIT_LIMIT), // You'll need to define this constant
            None,
            signer_prv_key.as_str().to_string(),
            None, // You might want to pass this as a parameter
        )
        .await
    }

    pub async fn compose_and_send(
        helius: Arc<Mutex<Helius>>,
        signer_prv_key: Arc<String>,
        max_sol_buy: u64,
        slippage_percent: u64,
        payload: String,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let pump_fun_tx = PumpFunTx::new(payload);

        let accounts = pump_fun_tx
            .accounts
            .as_ref()
            .ok_or("Unknown tx by meme accounts")?;

        let mint = Pubkey::from_str_const(accounts.mint.as_str());
        let bonding_curve = Pubkey::from_str_const(accounts.bonding_curve.as_str());
        let associated_bonding_curve =
            Pubkey::from_str_const(accounts.associated_bonding_curve.as_str());

        let signer = Keypair::from_base58_string(signer_prv_key.as_str());
        let signer_pub_key = signer.try_pubkey().unwrap();
        let user_ata = get_associated_token_address(&signer_pub_key, &mint);

        let ix_data = pump_fun_tx
            .ix_data
            .as_ref()
            .ok_or("Unknown tx by instruction data")?;

        let mut data = ix_data.instruction.clone();
        let is_buy = match ix_data.instruction.as_slice() {
            PUMP_FUN_ACTION_BUY => {
                let inner_ix_data = pump_fun_tx
                    .inner_ix_data
                    .as_ref()
                    .ok_or("Unknown tx by inner instructions")?;

                let copied_amount = inner_ix_data.amount as f64;
                let copied_sol = inner_ix_data.sol as f64;
                let computed_price = copied_sol / copied_amount;
                let amount = (max_sol_buy as f64 / computed_price) as u64;

                data.extend_from_slice(&amount.to_le_bytes());

                let fees = (max_sol_buy as f64 * PUMP_FUN_FEES).round() as u64;
                let slippage =
                    (max_sol_buy as f64 * slippage_percent as f64 / 100.0).round() as u64;
                let final_max_sol_buy = max_sol_buy + slippage + fees;
                data.extend_from_slice(&final_max_sol_buy.to_le_bytes());
                true
            }
            PUMP_FUN_ACTION_SELL => {
                let copied_amount = ix_data.amount as f64;
                let copied_min_sol_ouput = ix_data.sol as f64;
                let amount = copied_amount as u64;

                data.extend_from_slice(&amount.to_le_bytes());

                let min_sol_output =
                    (amount as f64 * (copied_min_sol_ouput / copied_amount)).round() as u64;
                let fees = (min_sol_output as f64 * PUMP_FUN_FEES).round() as u64;
                let slippage =
                    (min_sol_output as f64 * slippage_percent as f64 / 100.0).round() as u64;
                let final_min_sol_output = min_sol_output - slippage - fees;

                data.extend_from_slice(&final_min_sol_output.to_le_bytes());
                false
            }
            _ => return Err("Unknown instruction".into()),
        };

        Self::send_pump_fun_tx(
            helius,
            signer_prv_key,
            mint,
            bonding_curve,
            associated_bonding_curve,
            user_ata,
            data,
            is_buy,
        )
        .await
    }
}
