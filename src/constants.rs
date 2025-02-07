use solana_sdk::pubkey::Pubkey;

// Pump.fun accounts
pub const PUMP_FUN_PROGRAM: Pubkey =
    Pubkey::from_str_const("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const PUMP_FUN_GLOBAL: Pubkey =
    Pubkey::from_str_const("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const PUMP_FUN_FEE_RECIPIENT: Pubkey =
    Pubkey::from_str_const("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM");
pub const PUMP_EVENT_AUTHORITY: Pubkey =
    Pubkey::from_str_const("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
// Pump.fun program instructions
pub const PUMP_FUN_ACTION_BUY: &[u8] = &[102, 6, 61, 18, 1, 218, 235, 234];
pub const PUMP_FUN_ACTION_SELL: &[u8] = &[51, 230, 133, 164, 1, 127, 131, 173];
pub const PUMP_FUN_FEES: f64 = 0.01; // 1%

// Raydium constants

pub const RAYDIUM_LIQUIDITY_POOL_V4_PROGRAM: Pubkey =
    Pubkey::from_str_const("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
pub const RAYDIUM_AMM_AUTHORITY: Pubkey =
    Pubkey::from_str_const("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1");
pub const SERUM_PROGRAM: Pubkey =
    Pubkey::from_str_const("srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX");

pub const RAYDIUM_SWAP_BASE_IN_INSTRUCTION: u8 = 9;
pub const RAYDIUM_SWAP_BASE_OUT_INSTRUCTION: u8 = 11;
pub const RAYDIUM_ACCOUNTS_LEN_SWAP_BASE_IN: usize = 17;
pub const BOT_RAYDIUM_OPERATION_BUY: u8 = 1;
pub const BOT_RAYDIUM_OPERATION_SELL: u8 = 2;

// Solana constants
pub const SOL_DECIMALS: u64 = 10u64.pow(spl_token::native_mint::DECIMALS as u32);
pub const WSOL_MINT: Pubkey = Pubkey::from_str_const("So11111111111111111111111111111111111111112");
pub const DEFAULT_COMPUTE_UNIT_LIMIT: u32 = 100_000;
