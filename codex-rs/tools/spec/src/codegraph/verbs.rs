#[derive(Debug, Clone)]
pub struct VerbOutput {
    pub confident: bool,
    pub answer: String,
    pub hints: Vec<String>,
}
