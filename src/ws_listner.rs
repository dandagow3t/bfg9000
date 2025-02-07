#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(unused_must_use)]
mod bot;
mod constants;
mod errors;
mod fast_websocket_client;

use bot::{subscribe_raydium, PumpFunTxSend, RaydiumMemeTxSend};
use constants::SOL_DECIMALS;
use dotenv::dotenv;
use fast_websocket_client::{client, connect, OpCode};
use helius::{types::Cluster, Helius};
use std::{
    env,
    ops::Mul,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    print_bfg9000();

    dotenv().ok();

    let started_at = Instant::now();
    println!("\nStarted at: {:?}", started_at);

    // User wallet
    let singer_prv_key = Arc::new(env::var("signer_prv_key").unwrap());

    // Copy wallet(s)
    let copy_wallet = env::var("copy_wallet").unwrap();
    println!("Copy wallet(s): {copy_wallet}");

    // Max SOL put in a buy order
    let max_sol_buy = env::var("max_sol_buy")
        .unwrap()
        .parse::<f64>()
        .unwrap()
        .mul(SOL_DECIMALS as f64) as u64;
    println!("Max buy: {max_sol_buy} SOL");

    // Slippage
    let slippage_percent = env::var("slippage_percent")
        .unwrap()
        .parse::<f64>()
        .unwrap() as u64;

    // Max compute unit price in uLamports
    // let max_compute_unit_price = env::var("max_compute_unit_price_ulamports").unwrap().parse::<u64>().unwrap();

    // Helius client
    let helius = Arc::new(
        Helius::new_with_async_solana(
            env::var("helius_prod_api_key").unwrap().as_str(),
            Cluster::MainnetBeta,
        )
        .unwrap(),
    );

    // WSS URL
    let url = env::var("helius_prod_wss").unwrap();

    'reconnect_loop: loop {
        let future = connect(&url);
        let mut client: client::Online = match future.await {
            Ok(client) => {
                println!("connected");
                client
            }
            Err(e) => {
                eprintln!("Reconnecting from an Error: {e:?}");
                tokio::time::sleep(Duration::from_secs(10)).await;
                continue;
            }
        };

        // we can modify settings while running.
        // without pong, this app stops in about 15 minutes.(by the binance API spec.)
        client.set_auto_pong(true);

        // subscribe
        if let Err(e) = subscribe_raydium(&mut client, started_at).await {
            eprintln!("Reconnecting from an Error: {e:?}");
            let _ = client.send_close(&[]).await;
            // tokio::time::sleep(Duration::from_secs(10)).await;
            continue;
        };

        // message processing loop
        loop {
            let message = if let Ok(result) =
                tokio::time::timeout(Duration::from_secs(20), client.receive_frame()).await
            {
                match result {
                    Ok(message) => message,
                    Err(e) => {
                        eprintln!("Reconnecting from an Error: {e:?}");
                        let _ = client.send_close(&[]).await;
                        break; // break the message loop then reconnect
                    }
                }
            } else {
                println!("timeout");
                continue;
            };

            match message.opcode {
                OpCode::Text => {
                    let payload = match simdutf8::basic::from_utf8(message.payload.as_ref()) {
                        Ok(payload) => payload,
                        Err(e) => {
                            eprintln!("Reconnecting from an Error: {e:?}");
                            let _ = client.send_close(&[]).await;
                            break; // break the message loop then reconnect
                        }
                    };

                    // println!("\n---------------\n{payload}\n----------------------\n");
                    println!("\n>>>> got message >>>>\n");

                    let helius_clone = Arc::clone(&helius);
                    let signer_prv_key_clone = Arc::clone(&singer_prv_key);
                    let payload_clone = String::from(payload);

                    tokio::spawn(async move {
                        if let Err(e) = PumpFunTxSend::compose_and_send(
                            helius_clone,
                            signer_prv_key_clone,
                            max_sol_buy,
                            slippage_percent,
                            payload_clone,
                        )
                        .await
                        {
                            eprintln!("Error sending Pump.fun Meme Tx: {e:?}");
                        }
                    });

                    let helius_clone = Arc::clone(&helius);
                    let signer_prv_key_clone = Arc::clone(&singer_prv_key);
                    let payload_clone = String::from(payload);

                    tokio::spawn(async move {
                        if let Err(e) = RaydiumMemeTxSend::compose_and_send(
                            helius_clone,
                            signer_prv_key_clone,
                            max_sol_buy,
                            slippage_percent,
                            payload_clone,
                        )
                        .await
                        {
                            eprintln!("Error sending Raydium Meme Tx: {e:?}");
                        }
                    });
                }
                OpCode::Close => {
                    println!("{:?}", String::from_utf8_lossy(message.payload.as_ref()));
                    break 'reconnect_loop;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn print_bfg9000() {
    println!(
        r"____________ _____  _____ _   __
| ___ \  ___|  __ \|  _  | | / /
| |_/ / |_  | |  \/| |_| | |/ / 
| ___ \  _| | | __ \____ |    \ 
| |_/ / |   | |_\ \.___/ / |\  \
\____/\_|    \____/\____/\_| \_/"
    );
}
