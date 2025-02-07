use thiserror::Error;

#[derive(Error, Debug)]
pub enum BotError {
    #[error("An instruction's data contents was invalid")]
    InvalidInstructionData,
}
