use std::ops::Index;
use std::rc::Rc;
use std::str::Bytes;
use std::{fmt::Debug, iter};

type HashMap<K, V> = rustc_hash::FxHashMap<K, V>;

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    pub absolute: usize,
    pub line: usize,
    pub line_beginning: usize,
}

impl Position {
    pub fn new(absolute: usize, line: usize, line_beginning: usize) -> Self {
        assert!(line <= absolute);
        assert!(line_beginning <= absolute);
        Self {
            absolute,
            line,
            line_beginning,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Position,
    pub end: Position,
    pub file_name: Rc<str>,
    pub file_content: Rc<str>,
}

impl Debug for Span {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Span")
            .field("start", &self.start)
            .field("end", &self.end)
            .field("file_name", &self.file_name)
            .field("file_content [len]", &self.file_content.len())
            .finish()
    }
}

impl Span {
    pub fn new(start: Position, end: Position, file_name: Rc<str>, file_content: Rc<str>) -> Self {
        Self {
            start,
            end,
            file_name,
            file_content,
        }
    }

    pub fn start(&self) -> usize {
        self.start.absolute
    }

    pub fn end(&self) -> usize {
        self.end.absolute
    }

    pub fn file_name(&self) -> String {
        self.file_name.to_string()
    }

    pub fn file_content(&self) -> String {
        self.file_content.to_string()
    }
}

impl Index<Span> for str {
    type Output = str;
    fn index(&self, span: Span) -> &Self::Output {
        &self[span.start()..span.end()]
    }
}

impl Index<Span> for String {
    type Output = str;
    fn index(&self, span: Span) -> &Self::Output {
        &self[span.start()..span.end()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Spanned<T> {
    pub t: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(t: T, span: Span) -> Self {
        Self { t, span }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum SExpr {
    Atom(String, Span),
    List(Vec<SExpr>, Span),
    SpannedList(Spanned<Vec<SExpr>>),
    Variable(String, Span),
}

impl SExpr {
    pub fn atom<'a>(&'a self, vars: Option<&'a HashMap<String, SExpr>>) -> Option<&'a str> {
        match self {
            SExpr::Atom(s, _) => Some(s),
            SExpr::Variable(s, _) => vars.and_then(|vars| vars.get(s)?.atom(None)),
            _ => None,
        }
    }

    pub fn list<'a>(&'a self, vars: Option<&'a HashMap<String, SExpr>>) -> Option<&'a [SExpr]> {
        match self {
            SExpr::List(v, _) => Some(v),
            SExpr::SpannedList(Spanned { t, .. }) => Some(t),
            SExpr::Variable(s, _) => vars.and_then(|vars| vars.get(s)?.list(None)),
            _ => None,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            SExpr::Atom(_, span) => span.clone(),
            SExpr::List(_, span) => span.clone(),
            SExpr::SpannedList(Spanned { span, .. }) => span.clone(),
            SExpr::Variable(_, span) => span.clone(),
        }
    }
}

impl std::fmt::Debug for SExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SExpr::Atom(s, _) => write!(f, "{}", s),
            SExpr::List(v, _) => {
                write!(f, "(")?;
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", e)?;
                }
                write!(f, ")")
            }
            SExpr::SpannedList(Spanned { t, .. }) => {
                write!(f, "(")?;
                for (i, e) in t.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", e)?;
                }
                write!(f, ")")
            }
            SExpr::Variable(s, _) => write!(f, "${}", s),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SExprMetaData {
    Comment(Span),
    Whitespace(Span),
}

impl SExprMetaData {
    pub fn span(&self) -> Span {
        match self {
            SExprMetaData::Comment(span) => span.clone(),
            SExprMetaData::Whitespace(span) => span.clone(),
        }
    }
}

pub type TopLevel = Spanned<Vec<SExpr>>;

#[derive(Error, Debug, Diagnostic)]
#[error("Parse error")]
pub struct ParseError {
    #[source_code]
    pub src: String,

    #[label("Here")]
    pub span: SourceSpan,

    pub message: String,
}
