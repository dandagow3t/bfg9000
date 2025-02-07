use std::{error::Error, sync::Arc, time::Duration};

use helius::{
    error::HeliusError,
    jito::{JITO_API_URLS, JITO_TIP_ACCOUNTS},
    types::{
        CreateSmartTransactionConfig, GetPriorityFeeEstimateOptions, GetPriorityFeeEstimateRequest,
        GetPriorityFeeEstimateResponse, SmartTransaction, SmartTransactionConfig, Timeout,
    },
    Helius,
};
use rand::seq::SliceRandom;
use serde_json::Value;
use solana_client::{client_error::reqwest::StatusCode, rpc_config::RpcSendTransactionConfig};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    bs58::encode,
    commitment_config::CommitmentConfig,
    compute_budget::{self, ComputeBudgetInstruction},
    instruction::Instruction,
    message::{v0, VersionedMessage},
    signature::{Keypair, Signature},
    signer::Signer,
    transaction::{Transaction, VersionedTransaction},
};
// Serialize the transaction
use bincode::{serialize, ErrorKind};
use tokio::sync::Mutex;
pub trait GetSignature {
    // Returns the signature of the transaction
    fn get_signature(json: &Value) -> Option<String> {
        json["params"]["result"]["signature"]
            .as_str()
            .map(ToString::to_string)
    }
}

pub trait GetComputeData {
    /// Extracts the Compute Unit Limit and Compute Unit Price instructions
    /// from the transaction.
    fn get_compute_data(json: &Value) -> Option<(u32, u64)> {
        let instructions = json["params"]["result"]["transaction"]["transaction"]["message"]
            ["instructions"]
            .as_array()?;

        let mut compute_unit_limit = 0;
        let mut compute_unit_price = 0;

        for instruction in instructions {
            if instruction["programId"].as_str().unwrap() == compute_budget::id().to_string() {
                let ix_data = instruction["data"].as_str().unwrap();
                let decoded_bytes = bs58::decode(ix_data).into_vec().unwrap();
                match decoded_bytes[0] {
                    2 => {
                        compute_unit_limit =
                            u32::from_le_bytes(decoded_bytes[1..].try_into().unwrap())
                    }
                    3 => {
                        compute_unit_price =
                            u64::from_le_bytes(decoded_bytes[1..].try_into().unwrap())
                    }
                    _ => {}
                };
            }
        }

        Some((compute_unit_limit, compute_unit_price))
    }
}

pub trait GetTxCommon {
    fn pubkey_to_string(json: &Value) -> String {
        json.as_str()
            .map_or("not found".to_string(), ToString::to_string)
    }
}

pub trait SendSmartTx {
    /// Sends a smart transaction with a tip Tokio async compatible
    async fn bfg9000_send_smart_tx(
        helius: Arc<Mutex<Helius>>,
        mut instructions: Vec<Instruction>,
        units: Option<u32>,
        lookup_tables: Option<Vec<AddressLookupTableAccount>>,
        signer_prv_key: String,
        copied_tx_id: Option<String>,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let helius = helius.lock().await;
        // check if this needs to be a V0 tx
        let is_versioned: bool = lookup_tables.is_some();
        // instantiate signer
        let payer_and_signer = Arc::new(Keypair::from_base58_string(signer_prv_key.as_str()));
        let payer_and_signer_pubkey = payer_and_signer.pubkey();

        // determine tip for Jito
        // determine tip for Jito
        // TODO: Optimize tip
        let tip_amount = 10000; // 10k lamports;

        // select region for Jito
        let region = "Frankfurt";

        // create Jito api url
        let jito_region: &str = *JITO_API_URLS
            .get(region)
            .ok_or_else(|| HeliusError::InvalidInput("Invalid Jito region".to_string()))?;
        let jito_api_url_string: String = format!("{}/api/v1/bundles", jito_region);
        let jito_api_url: &str = jito_api_url_string.as_str();

        // create the smart transaction with tip
        // choose a random tip account
        let random_tip_account: &str = *JITO_TIP_ACCOUNTS.choose(&mut rand::thread_rng()).unwrap();
        // * add tip instruction to the instructions
        helius.add_tip_instruction(
            &mut instructions,
            payer_and_signer_pubkey,
            random_tip_account,
            tip_amount,
        );

        // ** create smart transaction
        // gets latest blockhash
        let (recent_blockhash, last_valid_block_height) = helius
            .async_connection()
            .unwrap()
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await
            .unwrap();

        // determine if we need to use a versioned transaction
        let mut legacy_transaction: Option<Transaction> = None;
        let mut versioned_transaction: Option<VersionedTransaction> = None;

        // Build the initial transaction based on whether lookup tables are present
        if is_versioned {
            let v0_message: v0::Message = v0::Message::try_compile(
                &payer_and_signer_pubkey,
                &instructions,
                lookup_tables.as_ref().unwrap(),
                recent_blockhash,
            )?;
            let versioned_message: VersionedMessage = VersionedMessage::V0(v0_message);

            // Sign the versioned transaction
            let signatures: Vec<Signature> =
                vec![payer_and_signer.try_sign_message(versioned_message.serialize().as_slice())?];

            versioned_transaction = Some(VersionedTransaction {
                signatures,
                message: versioned_message,
            });
        } else {
            // If no lookup tables are present, we build a regular transaction
            let mut tx: Transaction =
                Transaction::new_with_payer(&instructions, Some(&payer_and_signer_pubkey));
            tx.try_sign(&vec![Arc::clone(&payer_and_signer)], recent_blockhash)?;

            legacy_transaction = Some(tx);
        }

        // Serialize the transaction
        let serialized_tx: Vec<u8> = if let Some(tx) = &legacy_transaction {
            serialize(&tx).map_err(|e: Box<ErrorKind>| HeliusError::InvalidInput(e.to_string()))?
        } else if let Some(tx) = &versioned_transaction {
            serialize(&tx).map_err(|e: Box<ErrorKind>| HeliusError::InvalidInput(e.to_string()))?
        } else {
            return Err(Box::new(HeliusError::InvalidInput(
                "No transaction available".to_string(),
            )));
        };

        // Encode the transaction
        let transaction_base58: String = encode(&serialized_tx).into_string();

        // Get the priority fee estimate based on the serialized transaction
        let priority_fee_request: GetPriorityFeeEstimateRequest = GetPriorityFeeEstimateRequest {
            transaction: Some(transaction_base58),
            account_keys: None,
            options: Some(GetPriorityFeeEstimateOptions {
                recommended: Some(true),
                ..Default::default()
            }),
        };

        let priority_fee_estimate: GetPriorityFeeEstimateResponse = helius
            .rpc()
            .get_priority_fee_estimate(priority_fee_request)
            .await?;

        let priority_fee_recommendation: u64 =
            priority_fee_estimate
                .priority_fee_estimate
                .ok_or(HeliusError::InvalidInput(
                    "Priority fee estimate not available".to_string(),
                ))? as u64;

        println!(
            "priority fee recommendation: {}",
            priority_fee_recommendation
        );

        // // Override the priority fee recommendation
        // priority_fee_recommendation = 2_005_152;
        // println!(
        //     "overriden priority fee recommendation: {}",
        //     priority_fee_recommendation
        // );

        // Add the compute unit price instruction with the estimated fee
        let compute_budget_ix: Instruction =
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_recommendation);

        // Get the optimal compute units
        // TODO: Optimize compute units (simulated on RPC)

        println!("units: {}", units.unwrap());
        // Add the compute unit limit instruction with a margin
        let compute_units_ix: Instruction =
            ComputeBudgetInstruction::set_compute_unit_limit(units.unwrap());

        let mut final_instructions: Vec<Instruction> = vec![compute_budget_ix, compute_units_ix];

        final_instructions.extend(instructions);
        // serialize the transaction

        // Rebuild the transaction with the final instructions
        let smart_tx: SmartTransaction;
        if is_versioned {
            let v0_message: v0::Message = v0::Message::try_compile(
                &payer_and_signer_pubkey,
                &final_instructions,
                lookup_tables.as_ref().unwrap(),
                recent_blockhash,
            )?;
            let versioned_message: VersionedMessage = VersionedMessage::V0(v0_message);

            // Sign the versioned transaction
            let signatures: Vec<Signature> =
                vec![payer_and_signer.try_sign_message(versioned_message.serialize().as_slice())?];

            versioned_transaction = Some(VersionedTransaction {
                signatures,
                message: versioned_message,
            });
            smart_tx = SmartTransaction::Versioned(versioned_transaction.unwrap());
        } else {
            let mut tx: Transaction =
                Transaction::new_with_payer(&final_instructions, Some(&payer_and_signer_pubkey));
            tx.try_partial_sign(&vec![Arc::clone(&payer_and_signer)], recent_blockhash)?;

            legacy_transaction = Some(tx);

            smart_tx = SmartTransaction::Legacy(legacy_transaction.unwrap());
        }

        let serialized_transaction: Vec<u8> = match smart_tx {
            SmartTransaction::Legacy(tx) => serialize(&tx)
                .map_err(|e: Box<ErrorKind>| HeliusError::InvalidInput(e.to_string()))?,
            SmartTransaction::Versioned(tx) => serialize(&tx)
                .map_err(|e: Box<ErrorKind>| HeliusError::InvalidInput(e.to_string()))?,
        };
        let transaction_base58: String = encode(&serialized_transaction).into_string();

        // println!("Stop here!");
        // return Ok("Stop here".to_string());
        // Send the transaction as a Jito bundle
        let bundle_id: String = helius
            .send_jito_bundle(vec![transaction_base58], jito_api_url)
            .await?;

        println!("Bundle sent: {}", bundle_id);

        // Poll for confirmation status
        let timeout: Duration = Duration::from_secs(60);
        let interval: Duration = Duration::from_secs(5);
        let start: tokio::time::Instant = tokio::time::Instant::now();

        while start.elapsed() < timeout
            || helius
                .async_connection()
                .unwrap()
                .get_block_height()
                .await?
                <= last_valid_block_height
        {
            let bundle_statuses: Value = helius
                .get_bundle_statuses(vec![bundle_id.clone()], jito_api_url)
                .await?;

            if let Some(values) = bundle_statuses["result"]["value"].as_array() {
                if !values.is_empty() {
                    if let Some(status) = values[0]["confirmation_status"].as_str() {
                        if status == "confirmed" {
                            let tx_id = values[0]["transactions"][0].as_str().unwrap().to_string();

                            if copied_tx_id.is_some() {
                                println!(
                                    "| 1:: copied tx: https://solscan.io/tx/{}",
                                    copied_tx_id.unwrap()
                                );
                            }
                            println!(
                                "| 2:: bundle id https://explorer.jito.wtf/bundle/{}",
                                bundle_id
                            );
                            println!("| 3: priority fee: {}", priority_fee_recommendation);
                            println!("| 4::outgoing tx: https://solscan.io/tx/{}", tx_id);
                            return Ok(tx_id);
                        }
                    }
                }
            }

            tokio::time::sleep(interval).await;
        }
        println!(
            "Bundle failed to confirm, bundle id: https://explorer.jito.wtf/bundle/{}",
            bundle_id
        );

        Err(Box::new(HeliusError::Timeout {
            code: StatusCode::REQUEST_TIMEOUT,
            text: "xxx Error: Bundle failed to confirm within the timeout period".to_string(),
        }))
    }

    /// Sends a smart transaction with a tip using the Helius sdk
    async fn send_smart_tx(
        helius: &Helius,
        instructions: Vec<Instruction>,
        signer_prv_key: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let signer = Keypair::from_base58_string(signer_prv_key);
        let signers: Vec<Arc<dyn Signer>> = vec![Arc::new(signer)];

        let create_config = CreateSmartTransactionConfig {
            instructions,
            signers,
            lookup_tables: None,
            fee_payer: None,
            priority_fee_cap: None,
        };

        let config: SmartTransactionConfig = SmartTransactionConfig {
            create_config,
            send_options: RpcSendTransactionConfig {
                skip_preflight: false,
                preflight_commitment: None,
                encoding: None,
                max_retries: None,
                min_context_slot: None,
            },
            timeout: Timeout::default(),
        };

        // Send the optimized transaction with a 10k lamport tip using the Frankfurt region's API URL
        // TODO: Optimize tip
        // TOOD: Optimize method
        println!("sending tx with tip");
        match helius
            .send_smart_transaction_with_tip(config, Some(10000), Some("Frankfurt"))
            .await
        {
            Ok(bundle_id) => {
                println!("outgoing tx: https://solscan.io/tx/{}", bundle_id);
                Ok(bundle_id)
            }
            Err(e) => {
                eprintln!("Failed to send transaction: {:?}", e);
                Err(e.into())
            }
        }
    }

    /// Sends a transaction using the Helius sdk
    async fn send_tx(
        helius: Arc<Mutex<Helius>>,
        instructions: &[Instruction],
        signer_prv_key: &str,
    ) -> Result<String, Box<dyn Error>> {
        let helius = helius.lock().await;

        let signer = Keypair::from_base58_string(signer_prv_key);
        let signer_pubkey = signer.pubkey();
        let signers: Vec<Arc<dyn Signer>> = vec![Arc::new(signer)];
        let recent_blockhash = helius.connection().get_latest_blockhash()?;
        let transaction = Transaction::new_signed_with_payer(
            instructions,
            Some(&signer_pubkey),
            &signers,
            recent_blockhash,
        );

        match helius.connection().send_transaction_with_config(
            &transaction,
            RpcSendTransactionConfig {
                skip_preflight: false,
                preflight_commitment: None,
                encoding: None,
                max_retries: None,
                min_context_slot: None,
            },
        ) {
            Ok(tx_id) => {
                println!("outgoing tx: https://solscan.io/tx/{}", tx_id);
                Ok(tx_id.to_string())
            }
            Err(e) => {
                eprintln!("Failed to send transaction: {:?}", e);
                Err(e.into())
            }
        }
    }
}
