use crate::ast::*;

/// Multi-pass constant folding and expression propagation:
/// Evaluates compile-time constant arithmetic, string concatenations,
/// boolean logic, and constant ternary branches.
pub fn propagate_constants(file: &mut CuiFile) {
    for comp in &mut file.components {
        for prop in &mut comp.properties {
            if let Some(ref mut def) = prop.default_value {
                fold_expr(def);
            }
        }
        if let Some(ref mut root) = comp.root {
            fold_element(root);
        }
    }
}

fn fold_element(el: &mut ElementNode) {
    for prop in &mut el.props {
        fold_expr(&mut prop.value);
    }
    for handler in &mut el.handlers {
        for stmt in &mut handler.body {
            fold_stmt(stmt);
        }
    }
    for child in &mut el.children {
        match child {
            ChildItem::Element(inner) => fold_element(inner),
            ChildItem::For {
                iterable,
                key,
                body,
                ..
            } => {
                fold_expr(iterable);
                if let Some(k) = key {
                    fold_expr(k);
                }
                fold_element(body);
            }
            ChildItem::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                fold_expr(condition);
                fold_element(then_branch);
                if let Some(else_b) = else_branch {
                    fold_element(else_b);
                }
            }
        }
    }
}

fn fold_stmt(stmt: &mut Stmt) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            fold_expr(target);
            fold_expr(value);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
        } => {
            fold_expr(condition);
            for s in then_branch {
                fold_stmt(s);
            }
            for s in else_branch {
                fold_stmt(s);
            }
        }
        Stmt::Call { args, .. } => {
            for arg in args {
                fold_expr(arg);
            }
        }
        Stmt::Expr(e) => fold_expr(e),
    }
}

pub fn fold_expr(expr: &mut Expr) {
    // 1. Recurse down sub-expressions first
    match expr {
        Expr::Member(base, _) => fold_expr(base),
        Expr::Index(base, idx) => {
            fold_expr(base);
            fold_expr(idx);
        }
        Expr::Range { start, end, .. } => {
            fold_expr(start);
            fold_expr(end);
        }
        Expr::StructLit { fields, .. } => {
            for (_, val) in fields {
                fold_expr(val);
            }
        }
        Expr::Array(items) => {
            for item in items {
                fold_expr(item);
            }
        }
        Expr::Call(_, args) => {
            for arg in args {
                fold_expr(arg);
            }
        }
        Expr::MethodCall(recv, _, args) => {
            fold_expr(recv);
            for arg in args {
                fold_expr(arg);
            }
        }
        Expr::UnaryNeg(inner) => {
            fold_expr(inner);
            if let Expr::Int(v) = **inner {
                *expr = Expr::Int(-v);
                return;
            }
            if let Expr::Float(v) = **inner {
                *expr = Expr::Float(-v);
                return;
            }
        }
        Expr::UnaryNot(inner) => {
            fold_expr(inner);
            if let Expr::Bool(b) = **inner {
                *expr = Expr::Bool(!b);
                return;
            }
        }
        Expr::Binary(left, op, right) => {
            fold_expr(left);
            fold_expr(right);
            if let Some(folded) = fold_binary(left, *op, right) {
                *expr = folded;
                return;
            }
        }
        Expr::Ternary {
            cond,
            then_expr,
            else_expr,
        } => {
            fold_expr(cond);
            fold_expr(then_expr);
            fold_expr(else_expr);
            if let Expr::Bool(b) = **cond {
                if b {
                    *expr = (**then_expr).clone();
                } else {
                    *expr = (**else_expr).clone();
                }
                return;
            }
        }
        _ => {}
    }
}

fn fold_binary(left: &Expr, op: BinOp, right: &Expr) -> Option<Expr> {
    match (left, op, right) {
        // Integer arithmetic
        (Expr::Int(a), BinOp::Add, Expr::Int(b)) => Some(Expr::Int(a + b)),
        (Expr::Int(a), BinOp::Sub, Expr::Int(b)) => Some(Expr::Int(a - b)),
        (Expr::Int(a), BinOp::Mul, Expr::Int(b)) => Some(Expr::Int(a * b)),
        (Expr::Int(a), BinOp::Div, Expr::Int(b)) if *b != 0 => Some(Expr::Int(a / b)),

        // Float arithmetic
        (Expr::Float(a), BinOp::Add, Expr::Float(b)) => Some(Expr::Float(a + b)),
        (Expr::Float(a), BinOp::Sub, Expr::Float(b)) => Some(Expr::Float(a - b)),
        (Expr::Float(a), BinOp::Mul, Expr::Float(b)) => Some(Expr::Float(a * b)),
        (Expr::Float(a), BinOp::Div, Expr::Float(b)) if *b != 0.0 => Some(Expr::Float(a / b)),

        // Mixed Int / Float promotion
        (Expr::Int(a), BinOp::Add, Expr::Float(b)) => Some(Expr::Float(*a as f64 + b)),
        (Expr::Float(a), BinOp::Add, Expr::Int(b)) => Some(Expr::Float(a + *b as f64)),
        (Expr::Int(a), BinOp::Mul, Expr::Float(b)) => Some(Expr::Float(*a as f64 * b)),
        (Expr::Float(a), BinOp::Mul, Expr::Int(b)) => Some(Expr::Float(a * *b as f64)),

        // Length arithmetic
        (Expr::Length(a, u1), BinOp::Add, Expr::Length(b, u2)) if u1 == u2 => {
            Some(Expr::Length(a + b, *u1))
        }
        (Expr::Length(a, u1), BinOp::Sub, Expr::Length(b, u2)) if u1 == u2 => {
            Some(Expr::Length(a - b, *u1))
        }

        // String concatenation
        (Expr::String(a), BinOp::Add, Expr::String(b)) => {
            Some(Expr::String(format!("{a}{b}")))
        }

        // Boolean operations
        (Expr::Bool(true), BinOp::And, other) | (other, BinOp::And, Expr::Bool(true)) => {
            Some(other.clone())
        }
        (Expr::Bool(false), BinOp::And, _) | (_, BinOp::And, Expr::Bool(false)) => {
            Some(Expr::Bool(false))
        }
        (Expr::Bool(true), BinOp::Or, _) | (_, BinOp::Or, Expr::Bool(true)) => {
            Some(Expr::Bool(true))
        }
        (Expr::Bool(false), BinOp::Or, other) | (other, BinOp::Or, Expr::Bool(false)) => {
            Some(other.clone())
        }

        // Literal equality
        (Expr::Int(a), BinOp::Eq, Expr::Int(b)) => Some(Expr::Bool(a == b)),
        (Expr::Int(a), BinOp::Ne, Expr::Int(b)) => Some(Expr::Bool(a != b)),
        (Expr::Int(a), BinOp::Lt, Expr::Int(b)) => Some(Expr::Bool(a < b)),
        (Expr::Int(a), BinOp::Le, Expr::Int(b)) => Some(Expr::Bool(a <= b)),
        (Expr::Int(a), BinOp::Gt, Expr::Int(b)) => Some(Expr::Bool(a > b)),
        (Expr::Int(a), BinOp::Ge, Expr::Int(b)) => Some(Expr::Bool(a >= b)),

        (Expr::Float(a), BinOp::Eq, Expr::Float(b)) => Some(Expr::Bool(a == b)),
        (Expr::Float(a), BinOp::Ne, Expr::Float(b)) => Some(Expr::Bool(a != b)),
        (Expr::Float(a), BinOp::Lt, Expr::Float(b)) => Some(Expr::Bool(a < b)),
        (Expr::Float(a), BinOp::Le, Expr::Float(b)) => Some(Expr::Bool(a <= b)),
        (Expr::Float(a), BinOp::Gt, Expr::Float(b)) => Some(Expr::Bool(a > b)),
        (Expr::Float(a), BinOp::Ge, Expr::Float(b)) => Some(Expr::Bool(a >= b)),

        (Expr::String(a), BinOp::Eq, Expr::String(b)) => Some(Expr::Bool(a == b)),
        (Expr::String(a), BinOp::Ne, Expr::String(b)) => Some(Expr::Bool(a != b)),

        _ => None,
    }
}
