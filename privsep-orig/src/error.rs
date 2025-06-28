use thiserror::Error;

use crate::{controller::ControllerError, engine::EngineError, parser::ParserError};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("Controller error: {0}")]
    Controller(#[from] ControllerError),
    #[error("Parser error: {0}")]
    Parser(#[from] ParserError),
    #[error("Engine error: {0}")]
    Engine(#[from] EngineError),
}
