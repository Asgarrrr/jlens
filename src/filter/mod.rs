pub mod eval;
pub mod parse;

#[derive(Debug)]
pub enum FilterError {
    Parse(String),
    #[allow(dead_code)]
    Eval(String),
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterError::Parse(msg) => write!(f, "parse error: {}", msg),
            FilterError::Eval(msg) => write!(f, "eval error: {}", msg),
        }
    }
}
