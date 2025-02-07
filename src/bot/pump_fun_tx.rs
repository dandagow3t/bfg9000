use serde::Serialize;
use serde_json::{from_str, Value};
use solana_sdk::system_program;

use crate::bot::tx_common::GetSignature;
use crate::constants::{PUMP_FUN_FEE_RECIPIENT, PUMP_FUN_PROGRAM};

use super::tx_common::{GetComputeData, GetTxCommon};

#[derive(Debug, PartialEq, Serialize)]
pub struct PumpFunAccounts {
    pub mint: String,
    pub bonding_curve: String,
    pub associated_bonding_curve: String,
}

#[derive(Debug, Serialize)]
pub struct IxData {
    pub instruction: Vec<u8>,
    pub instruction_name: String,
    pub amount: u64,
    pub sol: u64,
}

#[derive(Debug, Serialize)]
pub struct InnerIxData {
    pub amount: u64,
    pub sol: u64,
    pub fee: u64,
}

#[derive(Debug, Serialize, Default)]
pub struct PumpFunTx {
    pub signature: Option<String>,
    pub accounts: Option<PumpFunAccounts>,
    pub ix_data: Option<IxData>,
    pub inner_ix_data: Option<InnerIxData>,
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
}

impl GetSignature for PumpFunTx {}

impl GetComputeData for PumpFunTx {}

impl GetTxCommon for PumpFunTx {}

impl PumpFunTx {
    pub fn new(payload: String) -> Self {
        let json: Value = from_str(payload.as_str()).expect("Invalid JSON");

        let signature = match Self::get_signature(&json) {
            Some(signature) => signature,
            None => {
                return Self::default();
            }
        };

        let accounts = match Self::get_accounts(&json) {
            Some(accounts) => accounts,
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: None,
                    ix_data: None,
                    inner_ix_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };

        let ix_data = match Self::get_pump_fun_data(&json) {
            Some((instruction, amount, sol)) => {
                let instruction_name = match instruction.as_slice() {
                    crate::constants::PUMP_FUN_ACTION_BUY => "buy",
                    crate::constants::PUMP_FUN_ACTION_SELL => "sell",
                    _ => "unknown",
                }
                .to_string();
                IxData {
                    instruction,
                    instruction_name,
                    amount,
                    sol,
                }
            }
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: Some(accounts),
                    ix_data: None,
                    inner_ix_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };

        let inner_ix_data = match Self::get_amounts(&json, &accounts) {
            Some((amount, sol, fee)) => InnerIxData { amount, sol, fee },
            None => {
                return Self {
                    signature: Some(signature),
                    accounts: Some(accounts),
                    ix_data: Some(ix_data),
                    inner_ix_data: None,
                    compute_unit_limit: 0,
                    compute_unit_price: 0,
                }
            }
        };

        let (compute_unit_limit, compute_unit_price) = match Self::get_compute_data(&json) {
            Some((compute_unit_limit, compute_unit_price)) => {
                (compute_unit_limit, compute_unit_price)
            }
            None => (0, 0),
        };

        Self {
            signature: Some(signature),
            accounts: Some(accounts),
            ix_data: Some(ix_data),
            inner_ix_data: Some(inner_ix_data),
            compute_unit_limit,
            compute_unit_price,
        }
    }

    /// Extracts the accounts needed for the Pump.fun transaction
    ///
    /// * `json` - The transaction JSON
    fn get_accounts(json: &Value) -> Option<PumpFunAccounts> {
        let instructions = match json["params"]["result"]["transaction"]["transaction"]["message"]
            ["instructions"]
            .as_array()
        {
            Some(instructions) => instructions,
            None => return None,
        };

        let pump_fun_instruction = match instructions.iter().find(|instruction| {
            instruction["programId"]
                .as_str()
                .map_or(false, |program_id| {
                    program_id == PUMP_FUN_PROGRAM.to_string()
                })
        }) {
            Some(pump_fun_instruction) => pump_fun_instruction,
            None => return None,
        };

        let accounts = match pump_fun_instruction["accounts"].as_array() {
            Some(accounts) => accounts,
            None => return None,
        };

        if accounts.len() != 12 {
            return None;
        }

        Some(PumpFunAccounts {
            mint: Self::pubkey_to_string(&accounts[2]),
            bonding_curve: Self::pubkey_to_string(&accounts[3]),
            associated_bonding_curve: Self::pubkey_to_string(&accounts[4]),
        })
    }

    /// Extracts the data from the Pump.fun instruction
    ///
    /// * `json` - The transaction JSON
    fn get_pump_fun_data(json: &Value) -> Option<(Vec<u8>, u64, u64)> {
        let instructions = match json["params"]["result"]["transaction"]["transaction"]["message"]
            ["instructions"]
            .as_array()
        {
            Some(instructions) => instructions,
            None => return None,
        };

        let pump_fun_instruction = match instructions.iter().find(|instruction| {
            instruction["programId"]
                .as_str()
                .map_or(false, |program_id| {
                    program_id == PUMP_FUN_PROGRAM.to_string()
                })
        }) {
            Some(pump_fun_instruction) => pump_fun_instruction,
            None => return None,
        };

        let ix_data = match pump_fun_instruction["data"].as_str() {
            Some(ix_data) => ix_data,
            None => return None,
        };
        let decoded_bytes = match bs58::decode(ix_data).into_vec().ok() {
            Some(decoded_bytes) => decoded_bytes,
            None => return None,
        };

        if decoded_bytes.len() < 24 {
            return None;
        }

        let instruction = decoded_bytes[..8].to_vec();
        let amount = match decoded_bytes[8..16].try_into().ok() {
            Some(amount) => u64::from_le_bytes(amount),
            None => 0,
        };
        let sol = match decoded_bytes[16..24].try_into().ok() {
            Some(sol) => u64::from_le_bytes(sol),
            None => 0,
        };

        Some((instruction, amount, sol))
    }

    /// Extracts the exact amounts swapped from the inner instructions
    /// so that, using those numbers, the exact price can be computed.
    ///
    /// * `json` - The transaction JSON
    /// * `meme_accounts` - The meme accounts
    fn get_amounts(json: &Value, accounts: &PumpFunAccounts) -> Option<(u64, u64, u64)> {
        let inner_instructions =
            match json["params"]["result"]["transaction"]["meta"]["innerInstructions"].as_array() {
                Some(inner_instructions) => inner_instructions,
                None => return None,
            };
        // println!("inner_instructions {:?}", inner_instructions);

        let mut spl_token_transfer_ix = None;
        let mut sol_transfer_ix = None;
        let mut fee_transfer_ix = None;

        // This works only for Buy transactions
        for inner_instruction in inner_instructions {
            match inner_instruction["instructions"].as_array() {
                Some(instructions) => {
                    for instruction in instructions {
                        if instruction["programId"]
                            .as_str()
                            .map_or(false, |program_id| {
                                program_id == spl_token::id().to_string()
                            })
                            && instruction["parsed"]["info"]["authority"]
                                .as_str()
                                .map_or(false, |authority| authority == accounts.bonding_curve)
                        {
                            spl_token_transfer_ix = Some(instruction);
                        }

                        if instruction["programId"]
                            .as_str()
                            .map_or(false, |program_id| {
                                program_id == system_program::id().to_string()
                            })
                            && instruction["parsed"]["info"]["destination"]
                                .as_str()
                                .map_or(false, |destination| destination == accounts.bonding_curve)
                        {
                            sol_transfer_ix = Some(instruction);
                        }

                        if instruction["programId"]
                            .as_str()
                            .map_or(false, |program_id| {
                                program_id == system_program::id().to_string()
                            })
                            && instruction["parsed"]["info"]["destination"]
                                .as_str()
                                .map_or(false, |destination| {
                                    destination == PUMP_FUN_FEE_RECIPIENT.to_string()
                                })
                        {
                            fee_transfer_ix = Some(instruction);
                        }
                    }
                }
                None => continue,
            }
        }

        let amount = match spl_token_transfer_ix {
            Some(ix) => match ix["parsed"]["info"]["amount"].as_str() {
                Some(amount) => match amount.parse::<u64>() {
                    Ok(amount) => amount,
                    Err(_) => 0,
                },
                None => 0,
            },
            None => 0,
        };

        let sol = match sol_transfer_ix {
            Some(ix) => match ix["parsed"]["info"]["lamports"].as_number() {
                Some(sol) => match sol.as_u64() {
                    Some(sol) => sol,
                    None => 0,
                },
                None => 0,
            },
            None => 0u64,
        };

        let fee = match fee_transfer_ix {
            Some(ix) => match ix["parsed"]["info"]["lamports"].as_number() {
                Some(fee) => match fee.as_u64() {
                    Some(fee) => fee,
                    None => 0,
                },
                None => 0,
            },
            None => 0,
        };

        Some((amount, sol, fee))
    }
}

#[cfg(test)]
mod tests {
    use crate::constants::{PUMP_FUN_ACTION_BUY, PUMP_FUN_ACTION_SELL};

    use super::*;

    const TEST_JSON_SELL: &str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":132442246888657,"result":{"transaction":{"transaction":{"signatures":["54aBiJz2eWtNEA6giPNz1MXPjmqFJGa3jnTN1XY6cUqqueJLxvn35ghBMtxn9iMvTnoHMxCa8eVbkbMMktZRFtV"],"message":{"accountKeys":[{"pubkey":"Hf7pVwBoMkPNrwfiLb5Pwx3f7Mtxerk2VokB3URBX6MV","writable":true,"signer":true,"source":"transaction"},{"pubkey":"AT9MPiGshbqzZCjwCX9RYaJKgHVRErhZHCfTWdh4h2eL","writable":true,"signer":false,"source":"transaction"},{"pubkey":"CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM","writable":true,"signer":false,"source":"transaction"},{"pubkey":"DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj","writable":true,"signer":false,"source":"transaction"},{"pubkey":"FQ9F48FX6g4h88ghvofUDS2d7SZumDSQUjwpLZVk7gXs","writable":true,"signer":false,"source":"transaction"},{"pubkey":"11111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ComputeBudget111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","writable":false,"signer":false,"source":"transaction"},{"pubkey":"4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf","writable":false,"signer":false,"source":"transaction"},{"pubkey":"8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","writable":false,"signer":false,"source":"transaction"},{"pubkey":"ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","writable":false,"signer":false,"source":"transaction"},{"pubkey":"Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1","writable":false,"signer":false,"source":"transaction"}],"recentBlockhash":"EpRp2ayjWjhZ5CVDPTF2zwBwVhBNXjSToSaMBMZh2EuN","instructions":[{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"3agR2zuU7Zf5","stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"E7NaRd","stackHeight":null},{"programId":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","accounts":["4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf","CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM","8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj","AT9MPiGshbqzZCjwCX9RYaJKgHVRErhZHCfTWdh4h2eL","FQ9F48FX6g4h88ghvofUDS2d7SZumDSQUjwpLZVk7gXs","Hf7pVwBoMkPNrwfiLb5Pwx3f7Mtxerk2VokB3URBX6MV","11111111111111111111111111111111","ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL","TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1","6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"],"data":"5jRcjdixRUDeyhgJJAYb4uu95HWFA2XTm","stackHeight":null}],"addressTableLookups":[]}},"meta":{"err":null,"status":{"Ok":null},"fee":862252,"preBalances":[8113912660,2039280,297813804298232,704219156,2039280,1,1141440,1,934087680,140530000,1461600,731913600,112000010],"postBalances":[8198299855,2039280,297813805159337,618108604,2039280,1,1141440,1,934087680,140530000,1461600,731913600,112000010],"innerInstructions":[{"index":2,"instructions":[{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"2948735678665","authority":"Hf7pVwBoMkPNrwfiLb5Pwx3f7Mtxerk2VokB3URBX6MV","destination":"AT9MPiGshbqzZCjwCX9RYaJKgHVRErhZHCfTWdh4h2eL","source":"FQ9F48FX6g4h88ghvofUDS2d7SZumDSQUjwpLZVk7gXs"},"type":"transfer"},"stackHeight":2},{"programId":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","accounts":["Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"],"data":"2K7nL28PxCW8ejnyCeuMpbWkdFHQ5EHuLmWeRP2E4aqyVUhQ1CyxNgMhBbMfSGSe7hvRd59jiDSciNsjUwBwGtnmUz9ps4ZgK54gYKQfm13ccMwN4brJy55cc6hypAA6QmGwbJ7VgSnBtrVpccrHYGvhAPD8MNpBaSgqMtPP1myLDnMutPforbsAgrAj","stackHeight":2}]}],"logMessages":["Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [1]","Program log: Instruction: Sell","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4645 of 290641 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [2]","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed 2003 of 282511 compute units","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success","Program data: vdt/007mYe5v9Y6xxZ9T2mLH6njijiD5tVu96hC53OGeR3idjxCvX1jxIQUAAAAAybhXjq4CAAAA937Fku8BAK/q5JtEsKYc5B18gmu5ZhpbXy5C4UxIDmjL7EpnAAAAAIx26CAHAAAA3ONwszm8AwCMysQkAAAAANxLXmeovQIA","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed 40918 of 319700 compute units","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success"],"preTokenBalances":[{"accountIndex":1,"mint":"8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","uiTokenAmount":{"uiAmount":975432204.131091,"decimals":6,"amount":"975432204131091","uiAmountString":"975432204.131091"},"owner":"DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":4,"mint":"8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","uiTokenAmount":{"uiAmount":2948735.678665,"decimals":6,"amount":"2948735678665","uiAmountString":"2948735.678665"},"owner":"Hf7pVwBoMkPNrwfiLb5Pwx3f7Mtxerk2VokB3URBX6MV","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"postTokenBalances":[{"accountIndex":1,"mint":"8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","uiTokenAmount":{"uiAmount":978380939.809756,"decimals":6,"amount":"978380939809756","uiAmountString":"978380939.809756"},"owner":"DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":4,"mint":"8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump","uiTokenAmount":{"uiAmount":0.0,"decimals":6,"amount":"0","uiAmountString":"0"},"owner":"Hf7pVwBoMkPNrwfiLb5Pwx3f7Mtxerk2VokB3URBX6MV","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"rewards":[],"computeUnitsConsumed":41218},"version":0},"signature":"54aBiJz2eWtNEA6giPNz1MXPjmqFJGa3jnTN1XY6cUqqueJLxvn35ghBMtxn9iMvTnoHMxCa8eVbkbMMktZRFtV","slot":304522881}}}"#;
    const TEST_JSON_BUY: &str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":8585665114076352,"result":{"transaction":{"transaction":{"signatures":["ArKpUgtPa32SBLP1uGRGFAWECuZYDZ37nrBv7DycZGTks7r17vdUYR6NxjqSeD6vQyVVyZSwKFfocz99foePMdF"],"message":{"accountKeys":[{"pubkey":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf","writable":true,"signer":true,"source":"transaction"},{"pubkey":"HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY","writable":true,"signer":false,"source":"transaction"},{"pubkey":"CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM","writable":true,"signer":false,"source":"transaction"},{"pubkey":"3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","writable":true,"signer":false,"source":"transaction"},{"pubkey":"jaeeUCUMKyjZudq2XEBhcB3wHNZrVU5gV33CUTgRwbK","writable":true,"signer":false,"source":"transaction"},{"pubkey":"Av1cBrij6Bn7BSpRsWcntW2MzK6d1t3ifTyG3zUZ9XDq","writable":true,"signer":false,"source":"transaction"},{"pubkey":"ComputeBudget111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"11111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","writable":false,"signer":false,"source":"transaction"},{"pubkey":"4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf","writable":false,"signer":false,"source":"transaction"},{"pubkey":"FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","writable":false,"signer":false,"source":"transaction"},{"pubkey":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","writable":false,"signer":false,"source":"transaction"},{"pubkey":"SysvarRent111111111111111111111111111111111","writable":false,"signer":false,"source":"transaction"},{"pubkey":"Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1","writable":false,"signer":false,"source":"transaction"}],"recentBlockhash":"8UgY4WTMfiWFHJFRG1BzB2pXYX5NBeeigFZ3xzWvcZjo","instructions":[{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"JxrTou","stackHeight":null},{"programId":"ComputeBudget111111111111111111111111111111","accounts":[],"data":"3ay2hEw4e3yH","stackHeight":null},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"HWEoBxYs7ssKuudEjzjmpfJVX7Dvi7wescFsVx2L5yoY","lamports":3000000,"source":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf"},"type":"transfer"},"stackHeight":null},{"programId":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","accounts":["4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf","CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM","FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","jaeeUCUMKyjZudq2XEBhcB3wHNZrVU5gV33CUTgRwbK","Av1cBrij6Bn7BSpRsWcntW2MzK6d1t3ifTyG3zUZ9XDq","9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf","11111111111111111111111111111111","TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","SysvarRent111111111111111111111111111111111","Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1","6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"],"data":"AJTQ2h9DXrC2uiZpVsCSqpaLYrDezLSZM","stackHeight":null}],"addressTableLookups":[]}},"meta":{"err":null,"status":{"Ok":null},"fee":495860,"preBalances":[369277771,258405587435,301193734533828,1231953,2039280,2039280,1,1,1141440,140530000,1461600,934087680,1009200,112000010],"postBalances":[365779567,258408587435,301193734533851,1234274,2039280,2039280,1,1,1141440,140530000,1461600,934087680,1009200,112000010],"innerInstructions":[{"index":3,"instructions":[{"program":"spl-token","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","parsed":{"info":{"amount":"83000000","authority":"3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","destination":"Av1cBrij6Bn7BSpRsWcntW2MzK6d1t3ifTyG3zUZ9XDq","source":"jaeeUCUMKyjZudq2XEBhcB3wHNZrVU5gV33CUTgRwbK"},"type":"transfer"},"stackHeight":2},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","lamports":2321,"source":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf"},"type":"transfer"},"stackHeight":2},{"program":"system","programId":"11111111111111111111111111111111","parsed":{"info":{"destination":"CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM","lamports":23,"source":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf"},"type":"transfer"},"stackHeight":2},{"programId":"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P","accounts":["Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1"],"data":"2K7nL28PxCW8ejnyCeuMpbXr5bU3AjAN97nQ8j4pgFeMi93tipBoZKgHhLdTC23GBoik3s6B7s6bdxNuCgTNH7jRyD9oLpdNNwHtZeXviYyc8oBDCFCRVqQ9QZyT2chaSjnKFinZXBHeuRiAvD55JDw9m9BHoX4c96NAbLNX72pnDHWSVc5h7rUrWgSb","stackHeight":2}]}],"logMessages":["Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program ComputeBudget111111111111111111111111111111 invoke [1]","Program ComputeBudget111111111111111111111111111111 success","Program 11111111111111111111111111111111 invoke [1]","Program 11111111111111111111111111111111 success","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [1]","Program log: Instruction: Buy","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]","Program log: Instruction: Transfer","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4645 of 18153 compute units","Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program 11111111111111111111111111111111 invoke [2]","Program 11111111111111111111111111111111 success","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [2]","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed 2003 of 6065 compute units","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success","Program data: vdt/007mYe7XrXq5It/Bo9+8lbjv0g3cdrbcK9uOr/41C1bNibapbxEJAAAAAAAAwHryBAAAAAABeTw3Umeaimte0hngnGRR2gWYx6WZyh2RP0+IOJoperS2RktnAAAAADK1I/wGAAAAWJfkQuPPAwAyCQAAAAAAAFj/0fZR0QIA","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P consumed 46299 of 48636 compute units","Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success"],"preTokenBalances":[{"accountIndex":4,"mint":"FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","uiTokenAmount":{"uiAmount":999999999.935,"decimals":6,"amount":"999999999935000","uiAmountString":"999999999.935"},"owner":"3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":5,"mint":"FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","uiTokenAmount":{"uiAmount":0.065,"decimals":6,"amount":"65000","uiAmountString":"0.065"},"owner":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"postTokenBalances":[{"accountIndex":4,"mint":"FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","uiTokenAmount":{"uiAmount":999999916.935,"decimals":6,"amount":"999999916935000","uiAmountString":"999999916.935"},"owner":"3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"},{"accountIndex":5,"mint":"FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump","uiTokenAmount":{"uiAmount":83.065,"decimals":6,"amount":"83065000","uiAmountString":"83.065"},"owner":"9AFb3BJTybJVvjWejqxstz9DUwYQxPepT94VCBi4escf","programId":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"}],"rewards":[],"computeUnitsConsumed":46749},"version":0},"signature":"ArKpUgtPa32SBLP1uGRGFAWECuZYDZ37nrBv7DycZGTks7r17vdUYR6NxjqSeD6vQyVVyZSwKFfocz99foePMdF","slot":304577356}}}"#;

    #[test]
    fn test_get_signature() {
        let json: Value = from_str(TEST_JSON_SELL).unwrap();
        let signature = PumpFunTx::get_signature(&json);
        assert_eq!(signature, Some("54aBiJz2eWtNEA6giPNz1MXPjmqFJGa3jnTN1XY6cUqqueJLxvn35ghBMtxn9iMvTnoHMxCa8eVbkbMMktZRFtV".to_owned()));
    }

    #[test]
    fn test_get_meme_accounts() {
        let json: Value = from_str(TEST_JSON_SELL).unwrap();
        let meme_accounts = PumpFunTx::get_accounts(&json).unwrap();
        assert_eq!(
            meme_accounts.mint,
            "8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump"
        );
        assert_eq!(
            meme_accounts.bonding_curve,
            "DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj"
        );
        assert_eq!(
            meme_accounts.associated_bonding_curve,
            "AT9MPiGshbqzZCjwCX9RYaJKgHVRErhZHCfTWdh4h2eL"
        );
    }

    #[test]
    fn test_get_pump_fun_data() {
        let json: Value = from_str(TEST_JSON_SELL).unwrap();
        let (instruction, amount, sol) = PumpFunTx::get_pump_fun_data(&json).unwrap();
        assert_eq!(instruction, PUMP_FUN_ACTION_SELL);
        assert_eq!(amount, 2948735678665);
        assert_eq!(sol, 1234);
    }

    #[test]
    fn test_new_sell() {
        let pump_fun_tx = PumpFunTx::new(String::from(TEST_JSON_SELL));
        assert_eq!(pump_fun_tx.signature, Some("54aBiJz2eWtNEA6giPNz1MXPjmqFJGa3jnTN1XY6cUqqueJLxvn35ghBMtxn9iMvTnoHMxCa8eVbkbMMktZRFtV".to_string()));
        let ix_data = pump_fun_tx.ix_data.unwrap();
        assert_eq!(ix_data.instruction, PUMP_FUN_ACTION_SELL);
        assert_eq!(ix_data.amount, 2948735678665);
        assert_eq!(ix_data.sol, 1234);
        let meme_accounts = pump_fun_tx.accounts.unwrap();
        assert_eq!(
            meme_accounts.mint,
            "8Y3RkHg21Gj1VBXJi7pcay4qeRqRrY3juZaBac64pump"
        );
        assert_eq!(
            meme_accounts.bonding_curve,
            "DMFahXyBj2uuRg42yGvoWmkM2XB7MoK7CJwzUCERrRyj"
        );
        assert_eq!(
            meme_accounts.associated_bonding_curve,
            "AT9MPiGshbqzZCjwCX9RYaJKgHVRErhZHCfTWdh4h2eL"
        );

        // No extraction happening on Sell trades
        let inner_ix_data = pump_fun_tx.inner_ix_data.unwrap();
        assert_eq!(inner_ix_data.amount, 0);
        assert_eq!(inner_ix_data.sol, 0);
        assert_eq!(inner_ix_data.fee, 0);
    }

    #[test]
    fn test_new_buy() {
        let pump_fun_tx = PumpFunTx::new(String::from(TEST_JSON_BUY));
        assert_eq!(pump_fun_tx.signature, Some("ArKpUgtPa32SBLP1uGRGFAWECuZYDZ37nrBv7DycZGTks7r17vdUYR6NxjqSeD6vQyVVyZSwKFfocz99foePMdF".to_string()));
        let ix_data = pump_fun_tx.ix_data.unwrap();
        assert_eq!(ix_data.instruction, PUMP_FUN_ACTION_BUY);
        assert_eq!(ix_data.amount, 83000000);
        assert_eq!(ix_data.sol, 2390);
        let meme_accounts = pump_fun_tx.accounts.unwrap();
        assert_eq!(
            meme_accounts.mint,
            "FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump"
        );
        assert_eq!(
            meme_accounts.bonding_curve,
            "3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T"
        );
        assert_eq!(
            meme_accounts.associated_bonding_curve,
            "jaeeUCUMKyjZudq2XEBhcB3wHNZrVU5gV33CUTgRwbK"
        );

        // No extraction happening on Sell trades
        let inner_ix_data = pump_fun_tx.inner_ix_data.unwrap();
        assert_eq!(inner_ix_data.amount, 83000000);
        assert_eq!(inner_ix_data.sol, 2321);
        assert_eq!(inner_ix_data.fee, 23);
    }

    #[test]
    fn test_get_signature_missing_field() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{}}}}}}"#; // Missing signature field
        let json: Value = from_str(json_str).unwrap();
        let signature = PumpFunTx::get_signature(&json);
        assert_eq!(signature, None);
    }

    #[test]
    fn test_get_signature_missing_field_2() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result23":{"transaction":{"transaction":{"message":{}}}}}}"#; // Missing signature field
        let json: Value = from_str(json_str).unwrap();
        let signature = PumpFunTx::get_signature(&json);
        assert_eq!(signature, None);
    }

    #[test]
    fn test_get_meme_accounts_missing_instructions() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{}}}}}}"#; // Missing instructions field
        let json: Value = from_str(json_str).unwrap();
        let meme_accounts = PumpFunTx::get_accounts(&json);
        assert_eq!(meme_accounts, None);
    }

    #[test]
    fn test_get_meme_accounts_invalid_program_id() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{"instructions":[{"programId":"invalid_program_id"}]}}}}}}"#; // Invalid program ID
        let json: Value = from_str(json_str).unwrap();
        let meme_accounts = PumpFunTx::get_accounts(&json);
        assert_eq!(meme_accounts, None);
    }

    #[test]
    fn test_get_meme_accounts_not_enough_accounts() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{"instructions":[{"programId":"pumpXFnZyLddLmBVHt88w3b2bXDfSXQXqcU2sMExVR8V","accounts":[1,2,3]}]}}}}}}"#; // Not enough accounts
        let json: Value = from_str(json_str).unwrap();
        let meme_accounts = PumpFunTx::get_accounts(&json);
        assert_eq!(meme_accounts, None);
    }

    #[test]
    fn test_get_pump_fun_data_missing_instructions() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{}}}}}}"#; // Missing instructions field
        let json: Value = from_str(json_str).unwrap();
        let pump_fun_data = PumpFunTx::get_pump_fun_data(&json);
        assert_eq!(pump_fun_data, None);
    }

    #[test]
    fn test_get_pump_fun_data_invalid_program_id() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{"instructions":[{"programId":"invalid_program_id"}]}}}}}}"#; // Invalid program ID
        let json: Value = from_str(json_str).unwrap();
        let pump_fun_data = PumpFunTx::get_pump_fun_data(&json);
        assert_eq!(pump_fun_data, None);
    }

    #[test]
    fn test_get_pump_fun_data_invalid_base58_data() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{"instructions":[{"programId":"pumpXFnZyLddLmBVHt88w3b2bXDfSXQXqcU2sMExVR8V","data":"invalid_base58"}]}}}}}}"#; // Invalid base58 data
        let json: Value = from_str(json_str).unwrap();
        let pump_fun_data = PumpFunTx::get_pump_fun_data(&json);
        assert_eq!(pump_fun_data, None);
    }

    #[test]
    fn test_get_pump_fun_data_not_enough_bytes() {
        let json_str = r#"{"jsonrpc":"2.0","method":"transactionNotification","params":{"subscription":177083441288849,"result":{"transaction":{"transaction":{"message":{"instructions":[{"programId":"pumpXFnZyLddLmBVHt88w3b2bXDfSXQXqcU2sMExVR8V","data":"12345678"}]}}}}}}"#; // Not enough bytes in data
        let json: Value = from_str(json_str).unwrap();
        let pump_fun_data = PumpFunTx::get_pump_fun_data(&json);
        assert_eq!(pump_fun_data, None);
    }

    #[test]
    fn test_get_amounts_basic() {
        let json: Value = from_str(TEST_JSON_BUY).unwrap();
        let meme_accounts = PumpFunTx::get_accounts(&json).unwrap();
        let (amount, sol, fee) = PumpFunTx::get_amounts(&json, &meme_accounts).unwrap();

        assert_eq!(amount, 83000000);
        assert_eq!(sol, 2321);
        assert_eq!(fee, 23);
    }

    #[test]
    fn test_get_amounts_missing_inner_instructions() {
        let json_str = r#"{"params":{"result":{"transaction":{"meta":{}}}}}"#; // Missing innerInstructions
        let json: Value = from_str(json_str).unwrap();
        let meme_accounts = PumpFunAccounts {
            mint: "mint".to_string(),
            bonding_curve: "bonding_curve".to_string(),
            associated_bonding_curve: "associated_bonding_curve".to_string(),
        };
        let amounts = PumpFunTx::get_amounts(&json, &meme_accounts);
        assert_eq!(amounts, None);
    }

    #[test]
    fn test_get_amounts_missing_spl_token_ix() {
        // Create a modified JSON where the spl-token instruction is missing
        let mut json: Value = from_str(TEST_JSON_BUY).unwrap();
        json["params"]["result"]["transaction"]["meta"]["innerInstructions"][0]["instructions"]
            .as_array_mut()
            .unwrap()
            .retain(|instruction| instruction["programId"] != spl_token::id().to_string());

        let meme_accounts = PumpFunTx::get_accounts(&json).unwrap();
        let amounts = PumpFunTx::get_amounts(&json, &meme_accounts);
        assert_eq!(amounts, Some((0, 2321, 23)));
    }

    #[test]
    fn test_get_amounts_invalid_amount_format() {
        // Create a modified JSON where the amount is not a valid u64
        let mut json: Value = from_str(TEST_JSON_SELL).unwrap();
        json["params"]["result"]["transaction"]["meta"]["innerInstructions"][0]["instructions"]
            [0]["parsed"]["info"]["amount"] = "invalid_amount".into();

        let meme_accounts = PumpFunTx::get_accounts(&json).unwrap();
        let amounts = PumpFunTx::get_amounts(&json, &meme_accounts);
        assert_eq!(amounts, Some((0, 0, 0)));
    }
}
