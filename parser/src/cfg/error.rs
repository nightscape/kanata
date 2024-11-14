use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;
use kanata_config::cfg::{
    error::{CfgError, MResult, ParseError, Result},
    sexpr::{Span, Spanned},
};

pub use kanata_config::cfg::error::{CfgError, MResult, ParseError, Result};

// Re-export the helper function
pub fn help(err_msg: impl AsRef<str>) -> String {
    format!("help: {}", err_msg.as_ref())
}

impl From<anyhow::Error> for ParseError {
    fn from(value: anyhow::Error) -> Self {
        Self::new_without_span(value.to_string())
    }
}

impl From<ParseError> for miette::Error {
    fn from(val: ParseError) -> Self {
        let diagnostic = CfgError {
            err_span: val
                .span
                .as_ref()
                .map(|s| SourceSpan::new(s.start().into(), (s.end() - s.start()).into())),
            help_msg: help(val.msg),
            file_name: val.span.as_ref().map(|s| s.file_name()),
            file_content: val.span.as_ref().map(|s| s.file_content()),
        };

        let report: miette::Error = diagnostic.into();

        if let Some(span) = val.span {
            report.with_source_code(NamedSource::new(span.file_name(), span.file_content()))
        } else {
            report
        }
    }
}

#[derive(Error, Debug, Diagnostic, Clone)]
#[error("Error in configuration")]
#[diagnostic()]
struct CfgError {
    // Snippets and highlights can be included in the diagnostic!
    #[label("Error here")]
    err_span: Option<SourceSpan>,
    #[help]
    help_msg: String,
    file_name: Option<String>,
    file_content: Option<String>,
}
