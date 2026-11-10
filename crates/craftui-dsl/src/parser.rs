use crate::ast::*;
use crate::cst::{SyntaxKind, SyntaxNode};
use crate::lexer::{Token, TokenKind, tokenize};
use rowan::GreenNodeBuilder;

pub struct ParseResult {
    pub file: CuiFile,
    pub cst: SyntaxNode,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn parse(source: &str) -> ParseResult {
    let tokens = tokenize(source);
    let mut parser = Parser {
        tokens,
        pos: 0,
        builder: GreenNodeBuilder::new(),
        diagnostics: Vec::new(),
    };
    let file = parser.parse_file();
    let green = parser.builder.finish();
    let cst = SyntaxNode::new_root(green);
    ParseResult {
        file,
        cst,
        diagnostics: parser.diagnostics,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    builder: GreenNodeBuilder<'static>,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn eat_trivia(&mut self) {
        while let Some(tok) = self.tokens.get(self.pos) {
            if tok.kind.is_trivia() {
                self.builder
                    .token(SyntaxKind::from(tok.kind).into(), &tok.text);
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<TokenKind> {
        let mut idx = self.pos;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                return Some(tok.kind);
            }
            idx += 1;
        }
        None
    }

    fn peek_nth(&self, n: usize) -> Option<TokenKind> {
        let mut idx = self.pos;
        let mut seen = 0usize;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                if seen == n {
                    return Some(tok.kind);
                }
                seen += 1;
            }
            idx += 1;
        }
        None
    }

    fn current_offset(&self) -> usize {
        let mut idx = self.pos;
        while let Some(tok) = self.tokens.get(idx) {
            if !tok.kind.is_trivia() {
                return tok.start;
            }
            idx += 1;
        }
        self.tokens.last().map(|t| t.end).unwrap_or(0)
    }

    fn bump(&mut self) -> Option<Token> {
        self.eat_trivia();
        let tok = self.tokens.get(self.pos)?.clone();
        self.builder
            .token(SyntaxKind::from(tok.kind).into(), &tok.text);
        self.pos += 1;
        Some(tok)
    }

    fn expect(&mut self, expected: TokenKind, msg: &str) -> Option<Token> {
        if self.peek() == Some(expected) {
            self.bump()
        } else {
            let off = self.current_offset();
            self.diagnostics.push(Diagnostic {
                span: Span::new(off, off + 1),
                severity: Severity::Error,
                message: msg.to_string(),
            });
            None
        }
    }

    fn eat_if(&mut self, kind: TokenKind) -> bool {
        if self.peek() == Some(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn parse_file(&mut self) -> CuiFile {
        self.builder.start_node(SyntaxKind::Root.into());
        let mut imports = Vec::new();
        let mut components = Vec::new();

        while let Some(kind) = self.peek() {
            match kind {
                TokenKind::KwImport => imports.push(self.parse_import()),
                TokenKind::KwExport | TokenKind::KwComponent => {
                    components.push(self.parse_component());
                }
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(tok) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(tok.start, tok.end),
                            severity: Severity::Error,
                            message: format!("unexpected top-level token '{}'", tok.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.eat_trivia();
        self.builder.finish_node();
        CuiFile {
            imports,
            components,
        }
    }

    fn parse_import(&mut self) -> ImportDecl {
        self.builder.start_node(SyntaxKind::ImportDecl.into());
        let start = self.current_offset();
        self.bump(); // import
        let mut symbols = Vec::new();
        if self.eat_if(TokenKind::LBrace) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RBrace {
                    break;
                }
                if pk == TokenKind::Ident
                    && let Some(t) = self.bump()
                {
                    symbols.push(t.text);
                } else if pk == TokenKind::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RBrace, "expected '}' in import list");
        }
        self.expect(TokenKind::KwFrom, "expected 'from' in import statement");
        let from_path = if self.peek() == Some(TokenKind::StringLit) {
            let t = self.bump().unwrap();
            unquote(&t.text)
        } else {
            self.expect(TokenKind::StringLit, "expected quoted module path");
            String::new()
        };
        self.eat_if(TokenKind::Semi);
        let end = self.current_offset();
        self.builder.finish_node();
        ImportDecl {
            symbols,
            from_path,
            span: Span::new(start, end),
        }
    }

    fn parse_component(&mut self) -> ComponentDecl {
        self.builder.start_node(SyntaxKind::ComponentDecl.into());
        let start = self.current_offset();
        let exported = self.eat_if(TokenKind::KwExport);
        self.expect(TokenKind::KwComponent, "expected 'component'");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected component name");
            "Unnamed".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after component name");

        let mut properties = Vec::new();
        let mut callbacks = Vec::new();
        let mut root = None;

        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            match pk {
                TokenKind::KwIn | TokenKind::KwOut | TokenKind::KwInOut | TokenKind::KwProperty => {
                    properties.push(self.parse_property_decl());
                }
                TokenKind::KwCallback => {
                    callbacks.push(self.parse_callback_decl());
                }
                TokenKind::Ident => {
                    root = Some(self.parse_element());
                }
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(t) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(t.start, t.end),
                            severity: Severity::Error,
                            message: format!("unexpected token '{}' in component body", t.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing component");
        let end = self.current_offset();
        self.builder.finish_node();
        ComponentDecl {
            exported,
            name,
            properties,
            callbacks,
            root,
            span: Span::new(start, end),
        }
    }

    fn parse_property_decl(&mut self) -> PropertyDecl {
        self.builder.start_node(SyntaxKind::PropertyDecl.into());
        let start = self.current_offset();
        let dir = match self.peek() {
            Some(TokenKind::KwIn) => {
                self.bump();
                PropertyDir::In
            }
            Some(TokenKind::KwOut) => {
                self.bump();
                PropertyDir::Out
            }
            Some(TokenKind::KwInOut) => {
                self.bump();
                PropertyDir::InOut
            }
            _ => PropertyDir::Private,
        };
        self.expect(TokenKind::KwProperty, "expected 'property'");
        self.expect(TokenKind::Lt, "expected '<' before property type");
        let ty = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected property type name");
            "unknown".into()
        };
        self.expect(TokenKind::Gt, "expected '>' after property type");
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected property name");
            "prop".into()
        };
        let default_value = if self.eat_if(TokenKind::Colon) {
            Some(self.parse_expr())
        } else {
            None
        };
        self.expect(TokenKind::Semi, "expected ';' after property declaration");
        let end = self.current_offset();
        self.builder.finish_node();
        PropertyDecl {
            dir,
            ty,
            name,
            default_value,
            span: Span::new(start, end),
        }
    }

    fn parse_callback_decl(&mut self) -> CallbackDecl {
        self.builder.start_node(SyntaxKind::CallbackDecl.into());
        let start = self.current_offset();
        self.bump(); // callback
        let name = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected callback name");
            "cb".into()
        };
        let mut params = Vec::new();
        if self.eat_if(TokenKind::LParen) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RParen {
                    break;
                }
                if pk == TokenKind::Ident {
                    params.push(self.bump().unwrap().text);
                } else if pk == TokenKind::Comma {
                    self.bump();
                } else {
                    break;
                }
            }
            self.expect(TokenKind::RParen, "expected ')' after callback params");
        }
        self.expect(TokenKind::Semi, "expected ';' after callback declaration");
        let end = self.current_offset();
        self.builder.finish_node();
        CallbackDecl {
            name,
            params,
            span: Span::new(start, end),
        }
    }

    fn parse_element(&mut self) -> ElementNode {
        self.builder.start_node(SyntaxKind::Element.into());
        let start = self.current_offset();
        let kind = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected element type name");
            "View".into()
        };
        self.expect(TokenKind::LBrace, "expected '{' after element name");

        let mut props = Vec::new();
        let mut handlers = Vec::new();
        let mut children = Vec::new();

        while let Some(pk) = self.peek() {
            if pk == TokenKind::RBrace {
                break;
            }
            match pk {
                TokenKind::KwFor => {
                    children.push(self.parse_for());
                }
                TokenKind::KwIf => {
                    children.push(self.parse_if());
                }
                TokenKind::Ident => match self.peek_nth(1) {
                    Some(TokenKind::LBrace) => {
                        children.push(ChildItem::Element(self.parse_element()));
                    }
                    Some(TokenKind::Colon) => {
                        props.push(self.parse_prop_binding(false));
                    }
                    Some(TokenKind::TwoWayBind) => {
                        props.push(self.parse_prop_binding(true));
                    }
                    Some(TokenKind::FatArrow) => {
                        handlers.push(self.parse_handler_binding());
                    }
                    _ => {
                        self.builder.start_node(SyntaxKind::ErrorNode.into());
                        if let Some(t) = self.bump() {
                            self.diagnostics.push(Diagnostic {
                                span: Span::new(t.start, t.end),
                                severity: Severity::Error,
                                message: format!("unexpected identifier '{}' in element", t.text),
                            });
                        }
                        self.builder.finish_node();
                    }
                },
                _ => {
                    self.builder.start_node(SyntaxKind::ErrorNode.into());
                    if let Some(t) = self.bump() {
                        self.diagnostics.push(Diagnostic {
                            span: Span::new(t.start, t.end),
                            severity: Severity::Error,
                            message: format!("unexpected token '{}' in element body", t.text),
                        });
                    }
                    self.builder.finish_node();
                }
            }
        }
        self.expect(TokenKind::RBrace, "expected '}' closing element");
        let end = self.current_offset();
        self.builder.finish_node();
        ElementNode {
            kind,
            props,
            handlers,
            children,
            span: Span::new(start, end),
        }
    }

    fn parse_prop_binding(&mut self, two_way: bool) -> PropBinding {
        let node_kind = if two_way {
            SyntaxKind::TwoWayAssign
        } else {
            SyntaxKind::PropertyAssign
        };
        self.builder.start_node(node_kind.into());
        let start = self.current_offset();
        let name = self.bump().map(|t| t.text).unwrap_or_default();
        self.bump(); // ':' or '<=>'
        let value = self.parse_expr();
        self.expect(TokenKind::Semi, "expected ';' after property binding");
        let end = self.current_offset();
        self.builder.finish_node();
        PropBinding {
            name,
            two_way,
            value,
            span: Span::new(start, end),
        }
    }

    fn parse_handler_binding(&mut self) -> HandlerBinding {
        self.builder.start_node(SyntaxKind::HandlerAssign.into());
        let start = self.current_offset();
        let event = self.bump().map(|t| t.text).unwrap_or_default();
        self.bump(); // '=>'
        let mut body = Vec::new();
        if self.eat_if(TokenKind::LBrace) {
            while let Some(pk) = self.peek() {
                if pk == TokenKind::RBrace {
                    break;
                }
                body.push(self.parse_stmt());
                self.eat_if(TokenKind::Semi);
            }
            self.expect(TokenKind::RBrace, "expected '}' closing handler block");
        } else {
            body.push(self.parse_stmt());
            self.eat_if(TokenKind::Semi);
        }
        let end = self.current_offset();
        self.builder.finish_node();
        HandlerBinding {
            event,
            body,
            span: Span::new(start, end),
        }
    }

    fn parse_stmt(&mut self) -> Stmt {
        if self.peek() == Some(TokenKind::Ident) && self.peek_nth(1) == Some(TokenKind::Eq) {
            let target = self.bump().unwrap().text;
            self.bump(); // '='
            let value = self.parse_expr();
            return Stmt::Assign { target, value };
        }
        let expr = self.parse_expr();
        if let Expr::Call(callee, args) = expr {
            Stmt::Call { callee, args }
        } else {
            Stmt::Expr(expr)
        }
    }

    fn parse_for(&mut self) -> ChildItem {
        self.builder.start_node(SyntaxKind::ForLoop.into());
        let start = self.current_offset();
        self.bump(); // for
        let item_var = if self.peek() == Some(TokenKind::Ident) {
            self.bump().unwrap().text
        } else {
            self.expect(TokenKind::Ident, "expected loop item identifier");
            "item".into()
        };
        let index_var = if self.eat_if(TokenKind::Comma) {
            if self.peek() == Some(TokenKind::Ident) {
                Some(self.bump().unwrap().text)
            } else {
                self.expect(TokenKind::Ident, "expected loop index identifier");
                None
            }
        } else {
            None
        };
        self.expect(TokenKind::KwIn, "expected 'in' in for loop");
        let iterable = self.parse_expr();
        self.expect(TokenKind::Colon, "expected ':' before for-loop element");
        let body = Box::new(self.parse_element());
        let end = self.current_offset();
        self.builder.finish_node();
        ChildItem::For {
            item_var,
            index_var,
            iterable,
            body,
            span: Span::new(start, end),
        }
    }

    fn parse_if(&mut self) -> ChildItem {
        self.builder.start_node(SyntaxKind::IfCond.into());
        let start = self.current_offset();
        self.bump(); // if
        let condition = self.parse_expr();
        self.expect(TokenKind::Colon, "expected ':' after if condition");
        let then_branch = Box::new(self.parse_element());
        let else_branch = if self.eat_if(TokenKind::KwElse) {
            self.eat_if(TokenKind::Colon);
            Some(Box::new(self.parse_element()))
        } else {
            None
        };
        let end = self.current_offset();
        self.builder.finish_node();
        ChildItem::If {
            condition,
            then_branch,
            else_branch,
            span: Span::new(start, end),
        }
    }

    fn parse_expr(&mut self) -> Expr {
        self.builder.start_node(SyntaxKind::ExprNode.into());
        let expr = self.parse_or();
        self.builder.finish_node();
        expr
    }

    fn parse_or(&mut self) -> Expr {
        let mut lhs = self.parse_and();
        while self.eat_if(TokenKind::OrOr) {
            let rhs = self.parse_and();
            lhs = Expr::Binary(Box::new(lhs), BinOp::Or, Box::new(rhs));
        }
        lhs
    }

    fn parse_and(&mut self) -> Expr {
        let mut lhs = self.parse_eq();
        while self.eat_if(TokenKind::AndAnd) {
            let rhs = self.parse_eq();
            lhs = Expr::Binary(Box::new(lhs), BinOp::And, Box::new(rhs));
        }
        lhs
    }

    fn parse_eq(&mut self) -> Expr {
        let mut lhs = self.parse_cmp();
        loop {
            let op = match self.peek() {
                Some(TokenKind::EqEq) => BinOp::Eq,
                Some(TokenKind::BangEq) => BinOp::Ne,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_cmp();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_cmp(&mut self) -> Expr {
        let mut lhs = self.parse_add();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Lt) => BinOp::Lt,
                Some(TokenKind::Gt) => BinOp::Gt,
                Some(TokenKind::Le) => BinOp::Le,
                Some(TokenKind::Ge) => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_add(&mut self) -> Expr {
        let mut lhs = self.parse_mul();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Plus) => BinOp::Add,
                Some(TokenKind::Minus) => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_mul(&mut self) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let op = match self.peek() {
                Some(TokenKind::Star) => BinOp::Mul,
                Some(TokenKind::Slash) => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_unary();
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        if self.eat_if(TokenKind::Bang) {
            return Expr::UnaryNot(Box::new(self.parse_unary()));
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        while self.eat_if(TokenKind::Dot) {
            if self.peek() == Some(TokenKind::Ident) {
                let field = self.bump().unwrap().text;
                expr = Expr::Member(Box::new(expr), field);
            }
        }
        expr
    }

    fn parse_primary(&mut self) -> Expr {
        match self.peek() {
            Some(TokenKind::KwTrue) => {
                self.bump();
                Expr::Bool(true)
            }
            Some(TokenKind::KwFalse) => {
                self.bump();
                Expr::Bool(false)
            }
            Some(TokenKind::ColorHex) => {
                let t = self.bump().unwrap();
                Expr::ColorHex(t.text)
            }
            Some(TokenKind::Dimension) => {
                let t = self.bump().unwrap();
                parse_dimension(&t.text)
            }
            Some(TokenKind::Float) => {
                let t = self.bump().unwrap();
                Expr::Float(t.text.parse().unwrap_or(0.0))
            }
            Some(TokenKind::Int) => {
                let t = self.bump().unwrap();
                Expr::Int(t.text.parse().unwrap_or(0))
            }
            Some(TokenKind::StringLit) => {
                let t = self.bump().unwrap();
                Expr::String(unquote(&t.text))
            }
            Some(TokenKind::Ident) => {
                let name = self.bump().unwrap().text;
                if self.eat_if(TokenKind::LParen) {
                    let mut args = Vec::new();
                    while let Some(pk) = self.peek() {
                        if pk == TokenKind::RParen {
                            break;
                        }
                        args.push(self.parse_expr());
                        if !self.eat_if(TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen, "expected ')' after arguments");
                    Expr::Call(name, args)
                } else {
                    Expr::Ident(name)
                }
            }
            Some(TokenKind::LBracket) => {
                self.bump();
                let mut items = Vec::new();
                while let Some(pk) = self.peek() {
                    if pk == TokenKind::RBracket {
                        break;
                    }
                    items.push(self.parse_expr());
                    if !self.eat_if(TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket, "expected ']' closing array");
                Expr::Array(items)
            }
            Some(TokenKind::LParen) => {
                self.bump();
                let e = self.parse_expr();
                self.expect(TokenKind::RParen, "expected ')'");
                e
            }
            _ => {
                let off = self.current_offset();
                self.diagnostics.push(Diagnostic {
                    span: Span::new(off, off + 1),
                    severity: Severity::Error,
                    message: "expected expression".into(),
                });
                Expr::Bool(false)
            }
        }
    }
}

fn unquote(s: &str) -> String {
    let inner = s
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .unwrap_or(s);
    inner.replace("\\\"", "\"").replace("\\n", "\n")
}

fn parse_dimension(s: &str) -> Expr {
    if let Some(num) = s.strip_suffix("dp") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Dp)
    } else if let Some(num) = s.strip_suffix("px") {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Px)
    } else if let Some(num) = s.strip_suffix('%') {
        Expr::Length(num.parse().unwrap_or(0.0), Unit::Pct)
    } else {
        Expr::Length(0.0, Unit::Dp)
    }
}
