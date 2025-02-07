#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(unused_must_use)]
mod agentic_tools;
mod bot;
mod constants;
mod db;
mod errors;
mod fast_websocket_client;
use anyhow::Result;
use dotenv::dotenv;
use helius::{types::Cluster, Helius};
use rig_mongodb::{MongoDbVectorIndex, SearchParams};
use std::{env, path::Path, sync::Arc};
use tokio::sync::Mutex;

use db::{Database, PumpFunCoinAccounts};
use fast_websocket_client::OpCode;

use agentic_tools::ToolPumpFunBuy;
use rig::{
    cli_chatbot::cli_chatbot,
    providers::{
        self,
        openai::{self, TEXT_EMBEDDING_ADA_002},
    },
};

use mongodb::{bson, options::ClientOptions, Client as MongoClient, Collection};

static DB_NAME: &str = "rig_knowledgebase";
static COLLECTION_NAME: &str = "context";
static VECTOR_INDEX_NAME: &str = "vector_index";

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    dotenv().ok();

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    // User wallet,
    // TODO: will have to move it to something more advanced
    let singer_prv_key = Arc::new(env::var("SIGNER_PRV_KEY").unwrap());

    // Helius non blocking RPC Client
    let helius = Arc::new(Mutex::new(
        Helius::new_with_async_solana(
            env::var("HELIUS_PROD_API_KEY").unwrap().as_str(),
            Cluster::MainnetBeta,
        )
        .unwrap(),
    ));

    // Initialize database
    let db = Database::new(Path::new("src/db/meme_coins.db")).await?;

    // TODO: remove this
    // TODO: provision information through another process
    if let Some(accounts) = db
        .get_pump_fun_coin_accounts_by_mint_address("FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump")
        .await?
    {
        println!("Accounts: {:?}", accounts);
    } else {
        println!("No accounts found");
        let accounts = PumpFunCoinAccounts {
            mint_address: "FWv5hiQqoUahjMyRFzz78q5ajmtwZ9vrn8tytgdFpump".to_string(),
            coin_name: "TEST_1".to_string(),
            bonding_curve: "3CtGMXMRJy4gwn6Fp6XzN6asErRqQd5pa4yCpBoqnN6T".to_string(),
            associated_bonding_curve: "jaeeUCUMKyjZudq2XEBhcB3wHNZrVU5gV33CUTgRwbK".to_string(),
            decimals: 6,
            price: 0.000000028, // price in SOL
        };

        db.add_pump_fun_coin_accounts(&accounts).await?;
    }

    // OpenAI client
    let openai_client =
        openai::Client::new(&env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"));

    // Tool for buying meme coins
    let tool_pump_fun_buy = ToolPumpFunBuy::new(helius, singer_prv_key, db.into());

    // Add the embeddings
    // Create the embedding model using OpenAI's text-embedding-ada-002
    let embedding_model = openai_client.embedding_model(TEXT_EMBEDDING_ADA_002);

    // Initialize MongoDB client
    let mongodb_connection_string = env::var("MONGODB_CONNECTION_STRING")
        .expect("MONGMONGODB_CONNECTION_STRINGODB_URI not set");
    let options = ClientOptions::parse(mongodb_connection_string)
        .await
        .expect("MongoDB connection string should be valid");

    let mongodb_client =
        MongoClient::with_options(options).expect("MongoDB client options should be valid");

    // Initialize MongoDB vector store
    let collection: Collection<bson::Document> =
        mongodb_client.database(DB_NAME).collection(COLLECTION_NAME);

    let index = MongoDbVectorIndex::new(
        collection,
        embedding_model,
        VECTOR_INDEX_NAME,
        SearchParams::new(),
    )
    .await?;

    // Create agent with a single context prompt and a single tool
    let agent = openai_client
        .agent(providers::openai::GPT_4O)
        .preamble("You are a Pump.fun trading assistant. Help users buy meme coins safely by using the provided tool. Always warn users about the risks of trading meme coins.")
        .dynamic_context(10, index)
        .max_tokens(8192)
        .tool(tool_pump_fun_buy)
        .build();

    println!(
        r"____________ _____  _____ _   __
    | ___ \  ___|  __ \|  _  | | / /
    | |_/ / |_  | |  \/| |_| | |/ / 
    | ___ \  _| | | __ \____ |    \ 
    | |_/ / |   | |_\ \.___/ / |\  \
    \____/\_|    \____/\____/\_| \_/",
    );

    // Start the chatbot
    cli_chatbot(agent).await?;

    Ok(())
}
