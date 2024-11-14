use std::ops::Index;
use std::rc::Rc;
use std::fmt::Debug;

type HashMap<K, V> = rustc_hash::FxHashMap<K, V>;

pub type TopLevel = Spanned<Vec<SExpr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// The position (since the beginning of the file), in bytes.
    pub absolute: usize,
    /// The number of newline characters since the beginning of the file.
    pub line: usize,
    /// The position of beginning of line, in bytes.
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

impl Default for Span {
    fn default() -> Self {
        Self {
            start: Position::default(),
            end: Position::default(),
            file_name: Rc::from(""),
            file_content: Rc::from(""),
        }
    }
}

impl Span {
    pub fn new(start: Position, end: Position, file_name: Rc<str>, file_content: Rc<str>) -> Span {
        assert!(start.absolute <= end.absolute);
        assert!(start.line <= end.line);
        Span {
            start,
            end,
            file_name,
            file_content,
        }
    }

    pub fn cover(&self, other: &Span) -> Span {
        assert!(self.file_name == other.file_name);

        let start: Position = if self.start() <= other.start() {
            self.start
        } else {
            other.start
        };

        let end: Position = if self.end() >= other.end() {
            self.end
        } else {
            other.end
        };

        Span::new(
            start,
            end,
            self.file_name.clone(),
            self.file_content.clone(),
        )
    }

    pub fn start(&self) -> usize {
        self.start.absolute
    }

    pub fn end(&self) -> usize {
        self.end.absolute
    }

    pub fn file_name(&self) -> String {
        self.file_name.clone().to_string()
    }

    pub fn file_content(&self) -> String {
        self.file_content.clone().to_string()
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
    pub fn new(t: T, span: Span) -> Spanned<T> {
        Spanned { t, span }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
/// I know this isn't the classic definition of an S-Expression which uses cons cell and atom, but
/// this is more convenient to work with (I find).
pub enum SExpr {
    Atom(Spanned<String>),
    List(Spanned<Vec<SExpr>>),
}

impl SExpr {
    pub fn atom<'a>(&'a self, vars: Option<&'a HashMap<String, SExpr>>) -> Option<&'a str> {
        match self {
            SExpr::Atom(a) => {
                let s = a.t.as_str();
                match (s.strip_prefix('$'), vars) {
                    (Some(varname), Some(vars)) => match vars.get(varname) {
                        Some(var) => {
                            #[cfg(feature = "lsp")]
                            super::LSP_VARIABLE_REFERENCES.with_borrow_mut(|refs| {
                                refs.push(varname, a.span.clone());
                            });
                            var.atom(Some(vars))
                        }
                        None => Some(s),
                    },
                    _ => Some(s),
                }
            }
            _ => None,
        }
    }

    pub fn list<'a>(&'a self, vars: Option<&'a HashMap<String, SExpr>>) -> Option<&'a [SExpr]> {
        match self {
            SExpr::List(l) => Some(&l.t),
            SExpr::Atom(a) => match (a.t.strip_prefix('$'), vars) {
                (Some(varname), Some(vars)) => match vars.get(varname) {
                    Some(var) => {
                        #[cfg(feature = "lsp")]
                        super::LSP_VARIABLE_REFERENCES.with_borrow_mut(|refs| {
                            refs.push(varname, a.span.clone());
                        });
                        var.list(Some(vars))
                    }
                    None => None,
                },
                _ => None,
            },
        }
    }

    pub fn span_list<'a>(
        &'a self,
        vars: Option<&'a HashMap<String, SExpr>>,
    ) -> Option<&'a Spanned<Vec<SExpr>>> {
        match self {
            SExpr::List(l) => Some(l),
            SExpr::Atom(a) => match (a.t.strip_prefix('$'), vars) {
                (Some(varname), Some(vars)) => match vars.get(varname) {
                    Some(var) => {
                        #[cfg(feature = "lsp")]
                        super::LSP_VARIABLE_REFERENCES.with_borrow_mut(|refs| {
                            refs.push(varname, a.span.clone());
                        });
                        var.span_list(Some(vars))
                    }
                    None => None,
                },
                _ => None,
            },
        }
    }

    pub fn span(&self) -> Span {
        match self {
            SExpr::Atom(a) => a.span.clone(),
            SExpr::List(l) => l.span.clone(),
        }
    }
}

impl std::fmt::Debug for SExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SExpr::Atom(a) => write!(f, "{}", &a.t),
            SExpr::List(l) => {
                write!(f, "(")?;
                for i in 0..l.t.len() - 1 {
                    write!(f, "{:?} ", &l.t[i])?;
                }
                if let Some(last) = &l.t.last() {
                    write!(f, "{last:?}")?;
                }
                write!(f, ")")?;
                Ok(())
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
/// Complementary to SExpr metadata items.
pub enum SExprMetaData {
    LineComment(Spanned<String>),
    BlockComment(Spanned<String>),
    Whitespace(Spanned<String>),
}

impl SExprMetaData {
    pub fn span(&self) -> Span {
        match self {
            Self::LineComment(x) => x.span.clone(),
            Self::BlockComment(x) => x.span.clone(),
            Self::Whitespace(x) => x.span.clone(),
        }
    }
}
