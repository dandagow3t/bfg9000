use crate::fast_websocket_client::client;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::constants::{PUMP_FUN_PROGRAM, RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM};

#[derive(Debug, Serialize)]
pub struct TransactionSubscribe(pub Value);

impl TransactionSubscribe {
    pub fn new(
        started_at: Instant,
        accounts_required: &[&str],
        accounts_excluded: &[&str],
    ) -> Self {
        Self(serde_json::json!(
        {
          "jsonrpc": "2.0",
          "id": started_at.elapsed().as_nanos(),
          "method": "transactionSubscribe",
          "params": [
              {
                "vote": false,
                "failed": false,
                "accountRequired": accounts_required,
                "accountExclude": accounts_excluded,
              },
              {
                "commitment": "processed",
                "encoding": "jsonParsed",
                "transaction_details": "full",
                "showRewards": true,
                "maxSupportedTransactionVersion": 0
              }
          ]
        }))
    }
}

#[allow(dead_code)]
pub async fn subscribe_pump_fun(
    client: &mut client::Online,
    started_at: Instant,
    copy_wallet: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::time::timeout(
        Duration::from_millis(0),
        client.send_json(
            &TransactionSubscribe::new(
                started_at,
                &[copy_wallet, PUMP_FUN_PROGRAM.to_string().as_ref()],
                &[],
            )
            .0,
        ),
    )
    .await??;
    Ok(())
}

pub async fn subscribe_raydium(
    client: &mut client::Online,
    started_at: Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::time::timeout(
        Duration::from_millis(0),
        client.send_json(
            &TransactionSubscribe::new(
                started_at,
                &[
                    RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM.to_string().as_ref(),
                    "6pURJRF6meemMHSdkuypCMrzUfDk1YKYa8MersBrpump",
                ],
                &[],
            )
            .0,
        ),
    )
    .await??;
    Ok(())
}
