#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(unused_must_use)]
mod agentic_tools;
mod bot;
mod constants;
mod errors;
mod fast_websocket_client;
use std::env;

use fast_websocket_client::OpCode;

use agentic_tools::ToolPumpFunBuy;
use rig::{
    agent::AgentBuilder,
    cli_chatbot::cli_chatbot,
    loaders::FileLoader,
    providers::openai::{self, GPT_4O},
};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    println!(
        r"____________ _____  _____ _   __
| ___ \  ___|  __ \|  _  | | / /
| |_/ / |_  | |  \/| |_| | |/ / 
| ___ \  _| | | __ \____ |    \ 
| |_/ / |   | |_\ \.___/ / |\  \
\____/\_|    \____/\____/\_| \_/"
    );

    let openai_client =
        openai::Client::new(&env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"));

    let model = openai_client.completion_model(GPT_4O);

    // Load in all the rust examples from multiple directories
    let examples = FileLoader::with_glob("rig-core/examples/*.rs")?
        .read_with_path()
        .ignore_errors()
        .into_iter()
        .chain(
            FileLoader::with_glob("src/bot/*.rs")?
                .read_with_path()
                .ignore_errors()
                .into_iter(),
        );

    // Create an agent with multiple context documents
    let agent = examples
        .fold(AgentBuilder::new(model), |builder, (path, content)| {
            builder.context(format!("Rust Example {:?}:\n{}", path, content).as_str())
        })
        .tool(ToolPumpFunBuy)
        .build();

    println!("Starting Gemini chatbot...");
    cli_chatbot(agent).await?;

    Ok(())
}
