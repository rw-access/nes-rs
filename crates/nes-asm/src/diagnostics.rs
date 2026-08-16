use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub length: usize,
}
impl Span {
    pub fn new(file: impl Into<String>, line: usize, column: usize, length: usize) -> Self {
        Self {
            file: file.into(),
            line,
            column,
            length,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}
impl Diagnostic {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.span.file.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(
                f,
                "{}:{}:{}: {}",
                self.span.file, self.span.line, self.span.column, self.message
            )
        }
    }
}
impl std::error::Error for Diagnostic {}
