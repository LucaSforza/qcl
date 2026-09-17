//! ASCII parser and name resolver for the QCL model language.
//!
//! Parsing intentionally produces source nodes containing names.  Resolution
//! is a separate operation so diagnostics can point at the original text and
//! callers can retain the source formula for display or later compilation.

use std::collections::{HashMap, HashSet};
use std::fmt;

use thiserror::Error;

use crate::ast::{CoalitionPredicate, Formula};
use crate::domain::{AgentId, AtomId, Coalition, StateId, StateSet};
use crate::model::{Effectivity, QclModel};
use crate::symbols::SymbolTable;

/// A half-open byte range in the source input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    /// Inclusive byte offset of the first source byte.
    pub start: usize,
    /// Exclusive byte offset immediately after the span.
    pub end: usize,
}

impl Span {
    const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A source node together with its location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spanned<T> {
    /// Location of the node in the original source.
    pub span: Span,
    /// Parsed node.
    pub node: T,
}

/// Errors produced by lexing, parsing, and name resolution.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ParseErrorKind {
    /// The token stream does not match the grammar.
    #[error("expected {expected}, found {found}")]
    Syntax { expected: String, found: String },
    /// A declaration repeats a name in one namespace.
    #[error("duplicate {kind} name `{name}`")]
    Duplicate { kind: String, name: String },
    /// A reference names an undeclared item.
    #[error("unknown {kind} name `{name}`")]
    Unknown { kind: String, name: String },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("{kind} at bytes {span:?}")]
pub struct ParseError {
    /// Source location associated with the error.
    pub span: Span,
    #[source]
    /// Specific syntax or resolution failure.
    pub kind: ParseErrorKind,
}

impl ParseError {
    fn syntax(span: Span, expected: impl Into<String>, found: impl Into<String>) -> Self {
        Self {
            span,
            kind: ParseErrorKind::Syntax {
                expected: expected.into(),
                found: found.into(),
            },
        }
    }

    fn duplicate(span: Span, kind: &str, name: &str) -> Self {
        Self {
            span,
            kind: ParseErrorKind::Duplicate {
                kind: kind.to_owned(),
                name: name.to_owned(),
            },
        }
    }

    fn unknown(span: Span, kind: &str, name: &str) -> Self {
        Self {
            span,
            kind: ParseErrorKind::Unknown {
                kind: kind.to_owned(),
                name: name.to_owned(),
            },
        }
    }
}

/// Unresolved coalition predicates retained by the source AST.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourcePredicate {
    /// Tested coalition is a subset of the named agents.
    Subset(Vec<String>),
    /// Tested coalition is a superset of the named agents.
    Superset(Vec<String>),
    /// Tested coalition has at least the given cardinality.
    SizeAtLeast(usize),
    /// Tested coalition contains exactly the named agents.
    Equals(Vec<String>),
    /// Tested coalition contains the named agent.
    Includes(String),
    /// Tested coalition excludes the named agent.
    Excludes(String),
    /// Every coalition.
    Any,
    /// Predicate negation.
    Not(Box<Spanned<SourcePredicate>>),
    /// Predicate conjunction.
    And(Box<Spanned<SourcePredicate>>, Box<Spanned<SourcePredicate>>),
    /// Predicate disjunction.
    Or(Box<Spanned<SourcePredicate>>, Box<Spanned<SourcePredicate>>),
}

/// A source predicate together with its byte span.
pub type SpannedPredicate = Spanned<SourcePredicate>;

/// Unresolved formula source AST.  Modality predicates remain source nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceFormula {
    /// Constant true.
    True,
    /// Constant false.
    False,
    /// An unresolved proposition name.
    Atom(String),
    /// Formula negation.
    Not(Box<Spanned<SourceFormula>>),
    /// Formula conjunction.
    And(Box<Spanned<SourceFormula>>, Box<Spanned<SourceFormula>>),
    /// Formula disjunction.
    Or(Box<Spanned<SourceFormula>>, Box<Spanned<SourceFormula>>),
    /// Material implication.
    Implies(Box<Spanned<SourceFormula>>, Box<Spanned<SourceFormula>>),
    /// Existential ability modality.
    Exists {
        /// Unresolved coalition predicate.
        predicate: Box<SpannedPredicate>,
        /// Formula under the modality.
        formula: Box<Spanned<SourceFormula>>,
    },
    /// Universal ability modality.
    Forall {
        /// Unresolved coalition predicate.
        predicate: Box<SpannedPredicate>,
        /// Formula under the modality.
        formula: Box<Spanned<SourceFormula>>,
    },
}

/// A source formula together with its byte span.
pub type SpannedFormula = Spanned<SourceFormula>;

impl SpannedPredicate {
    /// Resolve all agent names against a symbol table.
    ///
    /// # Errors
    ///
    /// Returns an error identifying the first unknown agent name.
    pub fn resolve(&self, agents: &SymbolTable<AgentId>) -> Result<CoalitionPredicate, ParseError> {
        resolve_predicate(self, agents)
    }
}

impl SpannedFormula {
    /// Resolve proposition and coalition names against the supplied tables.
    ///
    /// # Errors
    ///
    /// Returns an error identifying the first unknown proposition or agent.
    pub fn resolve(
        &self,
        agents: &SymbolTable<AgentId>,
        atoms: &SymbolTable<AtomId>,
    ) -> Result<Formula, ParseError> {
        resolve_formula(self, agents, atoms)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawValuation {
    state: String,
    properties: Vec<String>,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawEffectivity {
    state: String,
    agents: Vec<String>,
    outcomes: Vec<String>,
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq, Default)]
struct RawModel {
    agents: Vec<(String, Span)>,
    states: Vec<(String, Span)>,
    props: Vec<(String, Span)>,
    valuation: Vec<RawValuation>,
    effectivity: Vec<RawEffectivity>,
}

/// Parse and resolve a complete model declaration.
///
/// # Errors
///
/// Returns a syntax, duplicate-name, or unknown-name error.
pub fn parse_model(source: &str) -> Result<QclModel, ParseError> {
    let mut parser = Parser::new(source)?;
    let raw = parser.model()?;
    parser.expect(TokenKind::Eof)?;
    resolve_model(raw)
}

/// Parse a formula without resolving names.
///
/// # Errors
///
/// Returns a syntax error with the offending byte span.
pub fn parse_formula(source: &str) -> Result<SpannedFormula, ParseError> {
    let mut parser = Parser::new(source)?;
    let formula = parser.formula(0)?;
    parser.expect(TokenKind::Eof)?;
    Ok(formula)
}

/// Parse a coalition predicate without resolving names.
///
/// # Errors
///
/// Returns a syntax error with the offending byte span.
pub fn parse_predicate(source: &str) -> Result<SpannedPredicate, ParseError> {
    let mut parser = Parser::new(source)?;
    let predicate = parser.predicate(0)?;
    parser.expect(TokenKind::Eof)?;
    Ok(predicate)
}

fn resolve_model(raw: RawModel) -> Result<QclModel, ParseError> {
    let mut agents = SymbolTable::with_capacity(raw.agents.len());
    let mut states = SymbolTable::with_capacity(raw.states.len());
    let mut atoms = SymbolTable::with_capacity(raw.props.len());
    for (name, span) in raw.agents {
        agents
            .insert(name.clone())
            .map_err(|_| ParseError::duplicate(span, "agent", &name))?;
    }
    for (name, span) in raw.states {
        states
            .insert(name.clone())
            .map_err(|_| ParseError::duplicate(span, "state", &name))?;
    }
    for (name, span) in raw.props {
        atoms
            .insert(name.clone())
            .map_err(|_| ParseError::duplicate(span, "property", &name))?;
    }

    let mut valuation: HashMap<StateId, HashSet<AtomId>> = HashMap::new();
    for item in raw.valuation {
        let state = states
            .get(&item.state)
            .ok_or_else(|| ParseError::unknown(item.span, "state", &item.state))?;
        let mut values = HashSet::new();
        for property in item.properties {
            let atom = atoms
                .get(&property)
                .ok_or_else(|| ParseError::unknown(item.span, "property", &property))?;
            values.insert(atom);
        }
        valuation.insert(state, values);
    }

    let mut effectivity = Effectivity::new();
    for item in raw.effectivity {
        let state = states
            .get(&item.state)
            .ok_or_else(|| ParseError::unknown(item.span, "state", &item.state))?;
        let coalition = resolve_coalition(&item.agents, &agents, item.span)?;
        let outcomes = item
            .outcomes
            .iter()
            .map(|name| {
                states
                    .get(name)
                    .ok_or_else(|| ParseError::unknown(item.span, "state", name))
            })
            .collect::<Result<Vec<_>, _>>()?;
        effectivity.insert(state, coalition, StateSet::from_states(outcomes));
    }

    Ok(QclModel::new(agents, states, atoms, valuation, effectivity))
}

fn resolve_coalition(
    names: &[String],
    agents: &SymbolTable<AgentId>,
    span: Span,
) -> Result<Coalition, ParseError> {
    names
        .iter()
        .map(|name| {
            agents
                .get(name)
                .ok_or_else(|| ParseError::unknown(span, "agent", name))
        })
        .collect::<Result<Coalition, _>>()
}

fn resolve_predicate(
    source: &SpannedPredicate,
    agents: &SymbolTable<AgentId>,
) -> Result<CoalitionPredicate, ParseError> {
    let coalition = |names: &[String]| resolve_coalition(names, agents, source.span);
    Ok(match &source.node {
        SourcePredicate::Subset(names) => CoalitionPredicate::subset_eq(coalition(names)?),
        SourcePredicate::Superset(names) => CoalitionPredicate::superset_eq(coalition(names)?),
        SourcePredicate::SizeAtLeast(n) => CoalitionPredicate::geq(*n),
        SourcePredicate::Equals(names) => CoalitionPredicate::and(
            CoalitionPredicate::subset_eq(coalition(names)?),
            CoalitionPredicate::superset_eq(coalition(names)?),
        ),
        SourcePredicate::Includes(name) => {
            CoalitionPredicate::superset_eq(coalition(std::slice::from_ref(name))?)
        }
        SourcePredicate::Excludes(name) => CoalitionPredicate::negate(
            CoalitionPredicate::superset_eq(coalition(std::slice::from_ref(name))?),
        ),
        SourcePredicate::Any => CoalitionPredicate::geq(0),
        SourcePredicate::Not(value) => {
            CoalitionPredicate::negate(resolve_predicate(value, agents)?)
        }
        SourcePredicate::And(left, right) => CoalitionPredicate::and(
            resolve_predicate(left, agents)?,
            resolve_predicate(right, agents)?,
        ),
        SourcePredicate::Or(left, right) => CoalitionPredicate::or(
            resolve_predicate(left, agents)?,
            resolve_predicate(right, agents)?,
        ),
    })
}

fn resolve_formula(
    source: &SpannedFormula,
    agents: &SymbolTable<AgentId>,
    atoms: &SymbolTable<AtomId>,
) -> Result<Formula, ParseError> {
    Ok(match &source.node {
        SourceFormula::True => Formula::True,
        SourceFormula::False => Formula::False,
        SourceFormula::Atom(name) => Formula::atom(
            atoms
                .get(name)
                .ok_or_else(|| ParseError::unknown(source.span, "property", name))?,
        ),
        SourceFormula::Not(value) => Formula::negate(resolve_formula(value, agents, atoms)?),
        SourceFormula::And(left, right) => Formula::and(
            resolve_formula(left, agents, atoms)?,
            resolve_formula(right, agents, atoms)?,
        ),
        SourceFormula::Or(left, right) => Formula::or(
            resolve_formula(left, agents, atoms)?,
            resolve_formula(right, agents, atoms)?,
        ),
        SourceFormula::Implies(left, right) => Formula::implies(
            resolve_formula(left, agents, atoms)?,
            resolve_formula(right, agents, atoms)?,
        ),
        SourceFormula::Exists { predicate, formula } => Formula::exists(
            resolve_predicate(predicate, agents)?,
            resolve_formula(formula, agents, atoms)?,
        ),
        SourceFormula::Forall { predicate, formula } => Formula::forall(
            resolve_predicate(predicate, agents)?,
            resolve_formula(formula, agents, atoms)?,
        ),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Ident,
    Number,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LAngle,
    RAngle,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semi,
    Bang,
    Amp,
    Pipe,
    Arrow,
    Geq,
    Eof,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Ident => "identifier",
            Self::Number => "integer",
            Self::LBrace => "`{`",
            Self::RBrace => "`}`",
            Self::LParen => "`(`",
            Self::RParen => "`)`",
            Self::LAngle => "`<`",
            Self::RAngle => "`>`",
            Self::LBracket => "`[`",
            Self::RBracket => "`]`",
            Self::Comma => "`,`",
            Self::Colon => "`:`",
            Self::Semi => "`;`",
            Self::Bang => "`!`",
            Self::Amp => "`&`",
            Self::Pipe => "`|`",
            Self::Arrow => "`->`",
            Self::Geq => "`>=`",
            Self::Eof => "end of input",
        };
        f.write_str(value)
    }
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    text: String,
    span: Span,
}

struct Lexer<'a> {
    source: &'a [u8],
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            position: 0,
        }
    }

    #[allow(clippy::too_many_lines)]
    fn lex(mut self) -> Result<Vec<Token>, ParseError> {
        let mut tokens = Vec::new();
        while self.position < self.source.len() {
            let byte = self.source[self.position];
            if byte.is_ascii_whitespace() {
                self.position += 1;
                continue;
            }
            if byte == b'#' || (byte == b'/' && self.source.get(self.position + 1) == Some(&b'/')) {
                while self.position < self.source.len() && self.source[self.position] != b'\n' {
                    self.position += 1;
                }
                continue;
            }
            let start = self.position;
            let kind = match byte {
                b'{' => {
                    self.position += 1;
                    TokenKind::LBrace
                }
                b'}' => {
                    self.position += 1;
                    TokenKind::RBrace
                }
                b'(' => {
                    self.position += 1;
                    TokenKind::LParen
                }
                b')' => {
                    self.position += 1;
                    TokenKind::RParen
                }
                b'<' => {
                    self.position += 1;
                    TokenKind::LAngle
                }
                b'>' if self.source.get(self.position + 1) == Some(&b'=') => {
                    self.position += 2;
                    TokenKind::Geq
                }
                b'>' => {
                    self.position += 1;
                    TokenKind::RAngle
                }
                b'[' => {
                    self.position += 1;
                    TokenKind::LBracket
                }
                b']' => {
                    self.position += 1;
                    TokenKind::RBracket
                }
                b',' => {
                    self.position += 1;
                    TokenKind::Comma
                }
                b':' => {
                    self.position += 1;
                    TokenKind::Colon
                }
                b';' => {
                    self.position += 1;
                    TokenKind::Semi
                }
                b'!' => {
                    self.position += 1;
                    TokenKind::Bang
                }
                b'&' => {
                    self.position += 1;
                    TokenKind::Amp
                }
                b'|' => {
                    self.position += 1;
                    TokenKind::Pipe
                }
                b'-' if self.source.get(self.position + 1) == Some(&b'>') => {
                    self.position += 2;
                    TokenKind::Arrow
                }
                b'0'..=b'9' => {
                    self.position += 1;
                    while self
                        .source
                        .get(self.position)
                        .is_some_and(u8::is_ascii_digit)
                    {
                        self.position += 1;
                    }
                    TokenKind::Number
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    self.position += 1;
                    while self
                        .source
                        .get(self.position)
                        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    {
                        self.position += 1;
                    }
                    TokenKind::Ident
                }
                _ => {
                    return Err(ParseError::syntax(
                        Span::new(start, start + 1),
                        "token",
                        format!("`{}`", byte as char),
                    ));
                }
            };
            tokens.push(Token {
                kind,
                text: String::from_utf8_lossy(&self.source[start..self.position]).into_owned(),
                span: Span::new(start, self.position),
            });
        }
        tokens.push(Token {
            kind: TokenKind::Eof,
            text: String::new(),
            span: Span::new(self.position, self.position),
        });
        Ok(tokens)
    }
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn new(source: &str) -> Result<Self, ParseError> {
        Ok(Self {
            tokens: Lexer::new(source).lex()?,
            index: 0,
        })
    }

    fn current(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn bump(&mut self) -> Token {
        let token = self.tokens[self.index].clone();
        self.index += 1;
        token
    }
    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.current().kind == kind {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: TokenKind) -> Result<Token, ParseError> {
        if self.current().kind == kind {
            Ok(self.bump())
        } else {
            Err(ParseError::syntax(
                self.current().span,
                kind.to_string(),
                self.current().kind.to_string(),
            ))
        }
    }
    fn ident(&mut self, what: &str) -> Result<(String, Span), ParseError> {
        if self.current().kind != TokenKind::Ident {
            return Err(ParseError::syntax(
                self.current().span,
                what,
                self.current().kind.to_string(),
            ));
        }
        let token = self.bump();
        Ok((token.text, token.span))
    }

    fn model(&mut self) -> Result<RawModel, ParseError> {
        let (keyword, span) = self.ident("`model`")?;
        if keyword != "model" {
            return Err(ParseError::syntax(span, "`model`", keyword));
        }
        self.expect(TokenKind::LBrace)?;
        let mut model = RawModel::default();
        while self.current().kind != TokenKind::RBrace {
            let (section, section_span) = self.ident("model section")?;
            match section.as_str() {
                "agents" => model.agents = self.names_section("agent")?,
                "states" => model.states = self.names_section("state")?,
                "props" | "properties" | "atoms" => model.props = self.names_section("property")?,
                "valuation" => model.valuation = self.valuation_section()?,
                "effectivity" => model.effectivity = self.effectivity_section()?,
                _ => return Err(ParseError::syntax(section_span, "model section", section)),
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(model)
    }

    fn names_section(&mut self, _kind: &str) -> Result<Vec<(String, Span)>, ParseError> {
        self.expect(TokenKind::LBrace)?;
        let mut result = Vec::new();
        while self.current().kind != TokenKind::RBrace {
            let item = self.ident("name")?;
            result.push(item);
            if !self.eat(TokenKind::Comma) && self.current().kind != TokenKind::RBrace {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.expect(TokenKind::RBrace)?;
        self.expect(TokenKind::Semi)?;
        Ok(result)
    }

    fn name_list(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(TokenKind::LBrace)?;
        let mut result = Vec::new();
        while self.current().kind != TokenKind::RBrace {
            result.push(self.ident("name")?.0);
            if !self.eat(TokenKind::Comma) && self.current().kind != TokenKind::RBrace {
                self.expect(TokenKind::Comma)?;
            }
        }
        self.expect(TokenKind::RBrace)?;
        Ok(result)
    }

    fn valuation_section(&mut self) -> Result<Vec<RawValuation>, ParseError> {
        self.expect(TokenKind::LBrace)?;
        let mut result = Vec::new();
        while self.current().kind != TokenKind::RBrace {
            let (state, start) = self.ident("state name")?;
            self.expect(TokenKind::Colon)?;
            let properties = self.name_list()?;
            let end = self.expect(TokenKind::Semi)?.span.end;
            result.push(RawValuation {
                state,
                properties,
                span: Span::new(start.start, end),
            });
        }
        self.expect(TokenKind::RBrace)?;
        self.expect(TokenKind::Semi)?;
        Ok(result)
    }

    fn effectivity_section(&mut self) -> Result<Vec<RawEffectivity>, ParseError> {
        self.expect(TokenKind::LBrace)?;
        let mut result = Vec::new();
        while self.current().kind != TokenKind::RBrace {
            let (state, start) = self.ident("state name")?;
            self.eat(TokenKind::Comma);
            let agents = self.name_list()?;
            if !self.eat(TokenKind::Colon) {
                self.expect(TokenKind::Arrow)?;
            }
            let outcomes = self.name_list()?;
            let end = self.expect(TokenKind::Semi)?.span.end;
            result.push(RawEffectivity {
                state,
                agents,
                outcomes,
                span: Span::new(start.start, end),
            });
        }
        self.expect(TokenKind::RBrace)?;
        self.expect(TokenKind::Semi)?;
        Ok(result)
    }

    fn formula(&mut self, minimum: u8) -> Result<SpannedFormula, ParseError> {
        let mut left = self.formula_prefix()?;
        loop {
            let (precedence, right_assoc, kind) = match self.current().kind {
                TokenKind::Amp => (3, false, 0),
                TokenKind::Pipe => (2, false, 1),
                TokenKind::Arrow => (1, true, 2),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.bump();
            let right = self.formula(precedence + u8::from(!right_assoc))?;
            let span = Span::new(left.span.start, right.span.end);
            left = Spanned {
                span,
                node: match kind {
                    0 => SourceFormula::And(Box::new(left), Box::new(right)),
                    1 => SourceFormula::Or(Box::new(left), Box::new(right)),
                    _ => SourceFormula::Implies(Box::new(left), Box::new(right)),
                },
            };
        }
        Ok(left)
    }

    fn formula_prefix(&mut self) -> Result<SpannedFormula, ParseError> {
        if self.current().kind == TokenKind::Bang {
            let bang = self.bump();
            let value = self.formula_prefix()?;
            return Ok(Spanned {
                span: Span::new(bang.span.start, value.span.end),
                node: SourceFormula::Not(Box::new(value)),
            });
        }
        if self.eat(TokenKind::LParen) {
            let value = self.formula(0)?;
            self.expect(TokenKind::RParen)?;
            return Ok(value);
        }
        if self.current().kind == TokenKind::LAngle {
            let opener = self.bump();
            let predicate = self.predicate(0)?;
            self.expect(TokenKind::RAngle)?;
            let formula = self.formula_prefix()?;
            return Ok(Spanned {
                span: Span::new(opener.span.start, formula.span.end),
                node: SourceFormula::Exists {
                    predicate: Box::new(predicate),
                    formula: Box::new(formula),
                },
            });
        }
        if self.current().kind == TokenKind::LBracket {
            let opener = self.bump();
            let predicate = self.predicate(0)?;
            self.expect(TokenKind::RBracket)?;
            let formula = self.formula_prefix()?;
            return Ok(Spanned {
                span: Span::new(opener.span.start, formula.span.end),
                node: SourceFormula::Forall {
                    predicate: Box::new(predicate),
                    formula: Box::new(formula),
                },
            });
        }
        let token = self.current().clone();
        let (name, span) = self.ident("formula")?;
        let node = match name.as_str() {
            "true" => SourceFormula::True,
            "false" => SourceFormula::False,
            _ => SourceFormula::Atom(name),
        };
        let _ = token;
        Ok(Spanned { span, node })
    }

    fn predicate(&mut self, minimum: u8) -> Result<SpannedPredicate, ParseError> {
        let mut left = self.predicate_prefix()?;
        loop {
            let (precedence, kind) = match self.current().kind {
                TokenKind::Amp => (2, 0),
                TokenKind::Pipe => (1, 1),
                _ => break,
            };
            if precedence < minimum {
                break;
            }
            self.bump();
            let right = self.predicate(precedence + 1)?;
            let span = Span::new(left.span.start, right.span.end);
            left = Spanned {
                span,
                node: if kind == 0 {
                    SourcePredicate::And(Box::new(left), Box::new(right))
                } else {
                    SourcePredicate::Or(Box::new(left), Box::new(right))
                },
            };
        }
        Ok(left)
    }

    fn predicate_prefix(&mut self) -> Result<SpannedPredicate, ParseError> {
        if self.current().kind == TokenKind::Bang {
            let bang = self.bump();
            let value = self.predicate_prefix()?;
            return Ok(Spanned {
                span: Span::new(bang.span.start, value.span.end),
                node: SourcePredicate::Not(Box::new(value)),
            });
        }
        if self.eat(TokenKind::LParen) {
            let value = self.predicate(0)?;
            self.expect(TokenKind::RParen)?;
            return Ok(value);
        }
        let (name, start) = self.ident("predicate")?;
        let node = match name.as_str() {
            "any" => SourcePredicate::Any,
            "size" => {
                self.expect(TokenKind::Geq)?;
                let number = self.current().clone();
                self.expect(TokenKind::Number)?;
                SourcePredicate::SizeAtLeast(
                    number
                        .text
                        .parse()
                        .map_err(|_| ParseError::syntax(number.span, "integer", number.text))?,
                )
            }
            "subset" => SourcePredicate::Subset(self.coalition_call()?),
            "superset" => SourcePredicate::Superset(self.coalition_call()?),
            "equals" => SourcePredicate::Equals(self.coalition_call()?),
            "includes" => SourcePredicate::Includes(self.single_agent_call()?),
            "excludes" => SourcePredicate::Excludes(self.single_agent_call()?),
            _ => return Err(ParseError::syntax(start, "predicate", name)),
        };
        Ok(Spanned {
            span: Span::new(start.start, self.tokens[self.index - 1].span.end),
            node,
        })
    }

    fn single_agent_call(&mut self) -> Result<String, ParseError> {
        self.expect(TokenKind::LParen)?;
        let name = self.ident("agent name")?.0;
        self.expect(TokenKind::RParen)?;
        Ok(name)
    }

    fn coalition_call(&mut self) -> Result<Vec<String>, ParseError> {
        self.expect(TokenKind::LParen)?;
        let names = self.name_list()?;
        self.expect(TokenKind::RParen)?;
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r"
        model {
          agents { alice, bob };
          states { s0, s1 };
          props { ready, done };
          valuation { s0: {ready}; s1: {done}; };
          effectivity { s0, {alice}: {s1}; s0, {} -> {s0, s1}; };
        }
    ";

    #[test]
    fn parses_and_resolves_complete_model() {
        let model = parse_model(MODEL).expect("valid model");
        assert_eq!(model.agent_count(), 2);
        assert_eq!(model.state_count(), 2);
        assert_eq!(model.atom_count(), 2);
        assert!(model.is_true(StateId::new(0), AtomId::new(0)));
        assert_eq!(
            model
                .effectivity
                .outcomes(StateId::new(0), &Coalition::singleton(AgentId::new(0)))
                .map(<[_]>::len),
            Some(1)
        );
    }

    #[test]
    fn formula_precedence_and_modalities_are_preserved() {
        let formula = parse_formula("<size >= 1 & includes(alice)> ready -> [any] !done")
            .expect("valid formula");
        assert!(matches!(formula.node, SourceFormula::Implies(_, _)));
        let model = parse_model(MODEL).expect("valid model");
        let resolved = formula
            .resolve(&model.agents, &model.atoms)
            .expect("names resolve");
        assert!(matches!(resolved, Formula::Implies(_, _)));
    }

    #[test]
    fn derived_predicates_are_source_nodes_and_resolve() {
        let predicate =
            parse_predicate("equals({alice}) | excludes(bob)").expect("valid predicate");
        assert!(matches!(predicate.node, SourcePredicate::Or(_, _)));
        let model = parse_model(MODEL).expect("valid model");
        assert!(predicate.resolve(&model.agents).is_ok());
    }

    #[test]
    fn duplicate_and_unknown_names_have_structured_errors() {
        let duplicate = parse_model(
            "model { agents { alice, alice }; states {}; props {}; valuation {}; effectivity {}; }",
        )
        .unwrap_err();
        assert!(
            matches!(duplicate.kind, ParseErrorKind::Duplicate { ref kind, .. } if kind == "agent")
        );
        let unknown = parse_model(
            "model { agents {}; states {s}; props {}; valuation {x: {};}; effectivity {}; }",
        )
        .unwrap_err();
        assert!(
            matches!(unknown.kind, ParseErrorKind::Unknown { ref kind, .. } if kind == "state")
        );
    }

    #[test]
    fn malformed_input_reports_span() {
        let error = parse_formula("p &").unwrap_err();
        assert!(error.span.start > 0);
        assert!(matches!(error.kind, ParseErrorKind::Syntax { .. }));
    }
}
