# BFG9000 - Solana Meme Coin Trading Bot

BFG9000 is a Rust-based trading bot for Solana meme coins, currently supporting Pump.fun DEX with an AI-powered interface.

## Features

- AI-powered command interface using GPT-4
- Support for Pump.fun DEX trading
- Secure transaction handling with Jito MEV protection
- SQLite-based coin account management
- MongoDB vector store for context-aware AI responses
- Fast WebSocket client for real-time updates

## Prerequisites

- Rust toolchain (latest stable version)
- MongoDB instance
- Solana wallet with SOL
- API keys:
  - Helius API key
  - OpenAI API key

## Environment Variables

Create a `.env` file with the following variables:
HELIUS_PROD_API_KEY=your_helius_api_key
OPENAI_API_KEY=your_openai_api_key
SIGNER_PRV_KEY=your_wallet_private_key
MONGODB_CONNECTION_STRING=your_mongodb_connection_string

## Installation

1. Clone the repository:
```bash
git clone https://github.com/yourusername/bfg9000.git
cd bfg9000
```

2. Build the project:
```bash
cargo build --release
```

3. Run the bot:
```bash
cargo run --release
```

## Usage

The bot provides an interactive CLI interface where you can:

- Buy meme coins using natural language commands
- Get information about coins and their prices
- View transaction history
- Monitor real-time market updates

Example commands:
```
> Buy TEST_1 coin for 0.1 SOL with 1% slippage
> What is the current price of TEST_1?
```

## Safety Features

- Slippage protection
- Transaction simulation before execution
- Jito MEV protection for frontrunning resistance
- Maximum spend limits

## Warning

Trading meme coins involves significant risk. Only trade with funds you can afford to lose. This bot is provided as-is with no guarantees.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## Disclaimer

This software is for educational purposes only. Use at your own risk. The developers are not responsible for any financial losses incurred while using this software.