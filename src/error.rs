use thiserror::Error;

#[derive(Error, Debug)]
pub enum MetaError {
    #[error("API Error: {0}")]
    ApiError(#[from] reqwest::Error),
    
    #[error("JSON Error: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("Runtime Logic Error: {0}")]
    RuntimeError(String),
    
    #[error("Generation Failed: {0}")]
    GenerationFailed(String),
    
    #[error("Validation Failed: {0}")]
    ValidationFailed(String),
}