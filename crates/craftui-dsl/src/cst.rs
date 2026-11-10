use crate::lexer::TokenKind;
use rowan::Language;

/// Lossless Concrete Syntax Tree node & token kinds for `.cui` files (shared with `craftui-lsp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens (keep aligned with TokenKind variants)
    Whitespace = 0,
    LineComment,
    BlockComment,
    KwImport,
    KwFrom,
    KwExport,
    KwComponent,
    KwInOut,
    KwIn,
    KwOut,
    KwProperty,
    KwCallback,
    KwFor,
    KwIf,
    KwElse,
    KwTrue,
    KwFalse,
    TwoWayBind,
    FatArrow,
    EqEq,
    BangEq,
    Le,
    Ge,
    AndAnd,
    OrOr,
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Lt,
    Gt,
    Colon,
    Semi,
    Comma,
    Dot,
    Eq,
    Bang,
    Plus,
    Minus,
    Star,
    Slash,
    ColorHex,
    Dimension,
    Float,
    Int,
    StringLit,
    Ident,
    ErrorToken,

    // Composite CST Nodes
    Root,
    ImportDecl,
    ComponentDecl,
    PropertyDecl,
    CallbackDecl,
    Element,
    PropertyAssign,
    TwoWayAssign,
    HandlerAssign,
    ForLoop,
    IfCond,
    ExprNode,
    ErrorNode,
}

impl From<TokenKind> for SyntaxKind {
    fn from(tk: TokenKind) -> Self {
        match tk {
            TokenKind::Whitespace => Self::Whitespace,
            TokenKind::LineComment => Self::LineComment,
            TokenKind::BlockComment => Self::BlockComment,
            TokenKind::KwImport => Self::KwImport,
            TokenKind::KwFrom => Self::KwFrom,
            TokenKind::KwExport => Self::KwExport,
            TokenKind::KwComponent => Self::KwComponent,
            TokenKind::KwInOut => Self::KwInOut,
            TokenKind::KwIn => Self::KwIn,
            TokenKind::KwOut => Self::KwOut,
            TokenKind::KwProperty => Self::KwProperty,
            TokenKind::KwCallback => Self::KwCallback,
            TokenKind::KwFor => Self::KwFor,
            TokenKind::KwIf => Self::KwIf,
            TokenKind::KwElse => Self::KwElse,
            TokenKind::KwTrue => Self::KwTrue,
            TokenKind::KwFalse => Self::KwFalse,
            TokenKind::TwoWayBind => Self::TwoWayBind,
            TokenKind::FatArrow => Self::FatArrow,
            TokenKind::EqEq => Self::EqEq,
            TokenKind::BangEq => Self::BangEq,
            TokenKind::Le => Self::Le,
            TokenKind::Ge => Self::Ge,
            TokenKind::AndAnd => Self::AndAnd,
            TokenKind::OrOr => Self::OrOr,
            TokenKind::LBrace => Self::LBrace,
            TokenKind::RBrace => Self::RBrace,
            TokenKind::LParen => Self::LParen,
            TokenKind::RParen => Self::RParen,
            TokenKind::LBracket => Self::LBracket,
            TokenKind::RBracket => Self::RBracket,
            TokenKind::Lt => Self::Lt,
            TokenKind::Gt => Self::Gt,
            TokenKind::Colon => Self::Colon,
            TokenKind::Semi => Self::Semi,
            TokenKind::Comma => Self::Comma,
            TokenKind::Dot => Self::Dot,
            TokenKind::Eq => Self::Eq,
            TokenKind::Bang => Self::Bang,
            TokenKind::Plus => Self::Plus,
            TokenKind::Minus => Self::Minus,
            TokenKind::Star => Self::Star,
            TokenKind::Slash => Self::Slash,
            TokenKind::ColorHex => Self::ColorHex,
            TokenKind::Dimension => Self::Dimension,
            TokenKind::Float => Self::Float,
            TokenKind::Int => Self::Int,
            TokenKind::StringLit => Self::StringLit,
            TokenKind::Ident => Self::Ident,
            TokenKind::Error => Self::ErrorToken,
        }
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CuiLanguage {}

impl Language for CuiLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 <= SyntaxKind::ErrorNode as u16);
        // Safe: all discriminants from 0..=ErrorNode are valid contiguous repr(u16) variants.
        match raw.0 {
            0 => SyntaxKind::Whitespace,
            1 => SyntaxKind::LineComment,
            2 => SyntaxKind::BlockComment,
            3 => SyntaxKind::KwImport,
            4 => SyntaxKind::KwFrom,
            5 => SyntaxKind::KwExport,
            6 => SyntaxKind::KwComponent,
            7 => SyntaxKind::KwInOut,
            8 => SyntaxKind::KwIn,
            9 => SyntaxKind::KwOut,
            10 => SyntaxKind::KwProperty,
            11 => SyntaxKind::KwCallback,
            12 => SyntaxKind::KwFor,
            13 => SyntaxKind::KwIf,
            14 => SyntaxKind::KwElse,
            15 => SyntaxKind::KwTrue,
            16 => SyntaxKind::KwFalse,
            17 => SyntaxKind::TwoWayBind,
            18 => SyntaxKind::FatArrow,
            19 => SyntaxKind::EqEq,
            20 => SyntaxKind::BangEq,
            21 => SyntaxKind::Le,
            22 => SyntaxKind::Ge,
            23 => SyntaxKind::AndAnd,
            24 => SyntaxKind::OrOr,
            25 => SyntaxKind::LBrace,
            26 => SyntaxKind::RBrace,
            27 => SyntaxKind::LParen,
            28 => SyntaxKind::RParen,
            29 => SyntaxKind::LBracket,
            30 => SyntaxKind::RBracket,
            31 => SyntaxKind::Lt,
            32 => SyntaxKind::Gt,
            33 => SyntaxKind::Colon,
            34 => SyntaxKind::Semi,
            35 => SyntaxKind::Comma,
            36 => SyntaxKind::Dot,
            37 => SyntaxKind::Eq,
            38 => SyntaxKind::Bang,
            39 => SyntaxKind::Plus,
            40 => SyntaxKind::Minus,
            41 => SyntaxKind::Star,
            42 => SyntaxKind::Slash,
            43 => SyntaxKind::ColorHex,
            44 => SyntaxKind::Dimension,
            45 => SyntaxKind::Float,
            46 => SyntaxKind::Int,
            47 => SyntaxKind::StringLit,
            48 => SyntaxKind::Ident,
            49 => SyntaxKind::ErrorToken,
            50 => SyntaxKind::Root,
            51 => SyntaxKind::ImportDecl,
            52 => SyntaxKind::ComponentDecl,
            53 => SyntaxKind::PropertyDecl,
            54 => SyntaxKind::CallbackDecl,
            55 => SyntaxKind::Element,
            56 => SyntaxKind::PropertyAssign,
            57 => SyntaxKind::TwoWayAssign,
            58 => SyntaxKind::HandlerAssign,
            59 => SyntaxKind::ForLoop,
            60 => SyntaxKind::IfCond,
            61 => SyntaxKind::ExprNode,
            _ => SyntaxKind::ErrorNode,
        }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<CuiLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<CuiLanguage>;
